# Install the latest nomctl release on x64 Windows.
& {
    $ErrorActionPreference = 'Stop'
    if ($env:OS -ne 'Windows_NT') {
        throw 'This installer requires Windows. Use install.sh on macOS or Linux.'
    }
    $architecture = $env:PROCESSOR_ARCHITEW6432
    if (-not $architecture) { $architecture = $env:PROCESSOR_ARCHITECTURE }
    if ($architecture -ne 'AMD64') {
        throw 'Unsupported CPU architecture; Windows releases require x64.'
    }

    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    $release = Invoke-RestMethod 'https://api.github.com/repos/nominal-io/nominal-client-rs/releases/latest'
    if ($release.tag_name -notmatch '^nominal-v[0-9]') {
        throw "Unexpected latest release: $($release.tag_name)"
    }
    $name = "nomctl-$($release.tag_name -replace '^nominal-', '')-x86_64-pc-windows-msvc.zip"
    $asset = $release.assets | Where-Object { $_.name -eq $name } | Select-Object -First 1
    if (-not $asset) {
        throw "Release artifact $name is unavailable. It may still be building; try again shortly."
    }
    $destination = $env:NOMCTL_INSTALL_DIR
    if (-not $destination) { $destination = Join-Path $env:LOCALAPPDATA 'nomctl\bin' }
    $temporary = Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $temporary | Out-Null
    try {
        $archive = Join-Path $temporary 'nomctl.zip'
        Invoke-WebRequest -UseBasicParsing $asset.browser_download_url -OutFile $archive
        Expand-Archive -Path $archive -DestinationPath (Join-Path $temporary 'extracted')
        New-Item -ItemType Directory -Force -Path $destination | Out-Null
        Copy-Item (Join-Path $temporary 'extracted\nomctl.exe') (Join-Path $destination 'nomctl.exe') -Force
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        if ($destination -notin ($userPath -split ';')) {
            [Environment]::SetEnvironmentVariable('Path', "$destination;$userPath", 'User')
        }
        if ($destination -notin ($env:Path -split ';')) { $env:Path = "$destination;$env:Path" }
        Write-Host "Installed nomctl to $destination\nomctl.exe"
        Write-Host 'Run nomctl --version to verify. Restart other terminals to pick up the updated PATH.'
    } finally {
        Remove-Item -Recurse -Force $temporary
    }
}
