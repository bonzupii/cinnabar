#!/usr/bin/env bash
# Writes the Compose environment file that selects a checkout for the dev
# service, and, for a Windows linked worktree, the two proxy files that make
# its .git pointer meaningful inside Linux.  See CONTAINER_DEVELOPMENT.md.
#
# Unlike install-vscode-extension.sh this runs *on the host*, before any
# container exists -- it is the bootstrap that produces the file every later
# `docker compose --env-file` command reads.  So it is limited to host
# tooling: bash and Git, both of which this workflow already requires.
#
# Usage, from the repository root:
#
#   ./container/configure-worktree.sh --worktree "$PWD" --cache-key main
#   ./container/configure-worktree.sh --worktree /c/path/to/cinnabar-feature \
#     --cache-key feature
#
# --worktree defaults to the current directory and --cache-key to the
# worktree's directory name.
set -euo pipefail

SCRIPT_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"

die() {
  echo "$*" >&2
  exit 1
}

usage() {
  echo "Usage: ${0##*/} [--worktree PATH] [--cache-key KEY]" >&2
}

# Compose resolves bind sources against the host, so the paths written into
# the environment file must be host-native.  Under Git Bash $PWD is /c/...,
# which Docker rejects as a bind source; cygpath -m renders it C:/... with the
# forward slashes Compose wants.  On a Unix host there is nothing to convert.
to_host_path() {
  local path="$1"

  if command -v cygpath >/dev/null 2>&1; then
    path="$(cygpath -m -- "${path}")"
  fi

  case "${path}" in
    *'"'* | *$'\n'* | *$'\r'*)
      echo "Compose paths cannot contain quotes or newlines: ${path}" >&2
      return 1
      ;;
  esac

  printf '%s' "${path}"
}

# The inverse, for host paths that arrive from elsewhere -- Windows Git writes
# a C:/... pointer that this script has to stat and walk.
to_unix_path() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -u -- "$1"
  else
    printf '%s' "$1"
  fi
}

worktree="$(pwd -P)"
cache_key=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --worktree)
      [ "$#" -ge 2 ] || die "--worktree needs a path."
      worktree="$2"
      shift 2
      ;;
    --cache-key)
      [ "$#" -ge 2 ] || die "--cache-key needs a value."
      cache_key="$2"
      shift 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      usage
      die "Unrecognised argument: $1"
      ;;
  esac
done

[ -d "${worktree}" ] || die "No such directory: ${worktree}"
resolved_worktree="$(cd -- "${worktree}" && pwd -P)"
git_marker="${resolved_worktree}/.git"

if [ ! -e "${git_marker}" ]; then
  die "The selected directory is not a Git checkout: ${resolved_worktree}"
fi

if [ -z "${cache_key}" ]; then
  cache_key="$(basename -- "${resolved_worktree}")"
fi

cache_key="$(printf '%s' "${cache_key}" | tr '[:upper:]' '[:lower:]')"
case "${cache_key}" in
  [a-z0-9]*) ;;
  *) die "Cache key must start with a lowercase letter or digit." ;;
esac
case "${cache_key}" in
  *[!a-z0-9-]*) die "Cache key must contain only lowercase letters, digits, and hyphens." ;;
esac

configuration_root="${SCRIPT_ROOT}/local/${cache_key}"
mkdir -p -- "${configuration_root}"

worktree_host_path="$(to_host_path "${resolved_worktree}")"
environment_lines="CINNABAR_WORKTREE=\"${worktree_host_path}\"
CINNABAR_CACHE_KEY=${cache_key}"
compose_file_list="compose.dev.yaml"

if [ -d "${git_marker}" ]; then
  checkout_kind="main checkout"
else
  # One line, but Windows Git may end it with CRLF.
  pointer_line="$(tr -d '\r\n' < "${git_marker}")"
  case "${pointer_line}" in
    'gitdir: '*) ;;
    *) die "The linked-worktree .git file has an unsupported format." ;;
  esac

  git_admin_path="$(to_unix_path "${pointer_line#gitdir: }")"
  case "${git_admin_path}" in
    /*) ;;
    *) git_admin_path="${resolved_worktree}/${git_admin_path}" ;;
  esac
  [ -d "${git_admin_path}" ] || die "The .git pointer names no directory: ${git_admin_path}"

  git_admin_directory="$(cd -- "${git_admin_path}" && pwd -P)"
  worktrees_directory="$(dirname -- "${git_admin_directory}")"
  git_common="$(dirname -- "${worktrees_directory}")"
  git_admin="$(basename -- "${git_admin_directory}")"
  git_pointer="${configuration_root}/git-pointer"
  git_backlink="${configuration_root}/git-backlink"

  # Written by hand rather than copied so they carry LF endings and no BOM:
  # the container's Git reads them, and CRLF breaks pointer resolution.
  printf 'gitdir: /git-common/worktrees/%s\n' "${git_admin}" > "${git_pointer}"
  printf '/workspace/.git\n' > "${git_backlink}"

  git_common_host_path="$(to_host_path "${git_common}")"
  git_pointer_host_path="$(to_host_path "${git_pointer}")"
  git_backlink_host_path="$(to_host_path "${git_backlink}")"

  environment_lines="${environment_lines}
CINNABAR_GIT_COMMON=\"${git_common_host_path}\"
CINNABAR_GIT_POINTER=\"${git_pointer_host_path}\"
CINNABAR_GIT_BACKLINK=\"${git_backlink_host_path}\"
CINNABAR_GIT_ADMIN=${git_admin}"
  compose_file_list="${compose_file_list};compose.worktree.yaml"
  checkout_kind="linked worktree"
fi

# Compose reads COMPOSE_FILE from --env-file, so the selection travels with
# the file instead of being retyped as -f arguments.  This is the only thing
# that differed between a main checkout and a linked worktree, so carrying it
# here makes every command identical for both.  An explicit -f still wins,
# which keeps older invocations working.
#
# COMPOSE_PATH_SEPARATOR is pinned rather than left to the platform default
# (';' on Windows, ':' elsewhere) so one generated file means the same thing
# on either host.  ';' rather than ':' because a drive letter contains a colon.
environment_lines="${environment_lines}
COMPOSE_PATH_SEPARATOR=;
COMPOSE_FILE=${compose_file_list}"

environment_file="${configuration_root}/worktree.env"
printf '%s\n' "${environment_lines}" > "${environment_file}"

# The printed commands use a repository-relative --env-file because Compose
# resolves COMPOSE_FILE relative to the working directory too: both assume the
# repository root, and a relative path needs no host/Unix conversion to be
# usable from PowerShell, bash, and zsh alike.
relative_environment_file="container/local/${cache_key}/worktree.env"

echo "Configured ${checkout_kind} at ${resolved_worktree}"
echo "Environment file: ${environment_file}"
echo "Run these from the repository root:"
echo "Start: docker compose --env-file \"${relative_environment_file}\" up -d --build"
echo "Gate:  docker compose --env-file \"${relative_environment_file}\" exec dev nix develop --command ./pre_commit_check.sh"
