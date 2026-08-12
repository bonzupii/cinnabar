[CmdletBinding()]
param(
    [Parameter()]
    [string] $Worktree = (Get-Location).Path,

    [Parameter()]
    [string] $CacheKey
)

$ErrorActionPreference = "Stop"

function ConvertTo-ComposePath {
    param(
        [Parameter(Mandatory)]
        [string] $Path
    )

    if ($Path.Contains('"') -or $Path.Contains("`n") -or $Path.Contains("`r")) {
        throw "Compose paths cannot contain quotes or newlines."
    }

    return $Path.Replace('\', '/')
}

function Write-Utf8File {
    param(
        [Parameter(Mandatory)]
        [string] $Path,

        [Parameter(Mandatory)]
        [string] $Content
    )

    $Encoding = [System.Text.UTF8Encoding]::new($false)
    [System.IO.File]::WriteAllText($Path, $Content, $Encoding)
}

$ResolvedWorktree = (Resolve-Path -LiteralPath $Worktree).Path
$GitMarker = Join-Path $ResolvedWorktree ".git"

if (-not (Test-Path -LiteralPath $GitMarker)) {
    throw "The selected directory is not a Git checkout: $ResolvedWorktree"
}

if ([string]::IsNullOrWhiteSpace($CacheKey)) {
    $CacheKey = Split-Path -Leaf $ResolvedWorktree
}

$CacheKey = $CacheKey.ToLowerInvariant()
if ($CacheKey -notmatch '^[a-z0-9][a-z0-9-]*$') {
    throw "CacheKey must contain only lowercase letters, digits, and hyphens."
}

$ConfigurationRoot = Join-Path $PSScriptRoot "local/$CacheKey"
$CreatedDirectory = New-Item -ItemType Directory -Force -Path $ConfigurationRoot
if (-not $CreatedDirectory.PSIsContainer) {
    throw "Could not create configuration directory: $ConfigurationRoot"
}

$EnvironmentLines = [System.Collections.Generic.List[string]]::new()
$EnvironmentLines.Add("CINNABAR_WORKTREE=`"$(ConvertTo-ComposePath $ResolvedWorktree)`"")
$EnvironmentLines.Add("CINNABAR_CACHE_KEY=$CacheKey")
$ComposeFiles = "-f compose.dev.yaml"

if ((Get-Item -Force -LiteralPath $GitMarker).PSIsContainer) {
    $CheckoutKind = "main checkout"
} else {
    $PointerLine = [System.IO.File]::ReadAllText($GitMarker).Trim()
    if (-not $PointerLine.StartsWith("gitdir: ", [System.StringComparison]::Ordinal)) {
        throw "The linked-worktree .git file has an unsupported format."
    }

    $GitAdminPath = $PointerLine.Substring(8)
    if ([System.IO.Path]::IsPathRooted($GitAdminPath)) {
        $GitAdminDirectory = [System.IO.Path]::GetFullPath($GitAdminPath)
    } else {
        $GitAdminDirectory = [System.IO.Path]::GetFullPath((Join-Path $ResolvedWorktree $GitAdminPath))
    }
    $WorktreesDirectory = Split-Path -Parent $GitAdminDirectory
    $GitCommon = Split-Path -Parent $WorktreesDirectory
    $GitAdmin = Split-Path -Leaf $GitAdminDirectory
    $GitPointer = Join-Path $ConfigurationRoot "git-pointer"
    $GitBacklink = Join-Path $ConfigurationRoot "git-backlink"

    Write-Utf8File -Path $GitPointer -Content "gitdir: /git-common/worktrees/$GitAdmin`n"
    Write-Utf8File -Path $GitBacklink -Content "/workspace/.git`n"

    $EnvironmentLines.Add("CINNABAR_GIT_COMMON=`"$(ConvertTo-ComposePath $GitCommon)`"")
    $EnvironmentLines.Add("CINNABAR_GIT_POINTER=`"$(ConvertTo-ComposePath $GitPointer)`"")
    $EnvironmentLines.Add("CINNABAR_GIT_BACKLINK=`"$(ConvertTo-ComposePath $GitBacklink)`"")
    $EnvironmentLines.Add("CINNABAR_GIT_ADMIN=$GitAdmin")
    $ComposeFiles = "$ComposeFiles -f compose.worktree.yaml"
    $CheckoutKind = "linked worktree"
}

$EnvironmentFile = Join-Path $ConfigurationRoot "worktree.env"
Write-Utf8File -Path $EnvironmentFile -Content (($EnvironmentLines -join "`n") + "`n")

Write-Output "Configured $CheckoutKind at $ResolvedWorktree"
Write-Output "Environment file: $EnvironmentFile"
Write-Output "Start: docker compose --env-file `"$EnvironmentFile`" $ComposeFiles up -d --build"
Write-Output "Gate:  docker compose --env-file `"$EnvironmentFile`" $ComposeFiles exec dev nix develop --command ./pre_commit_check.sh"
