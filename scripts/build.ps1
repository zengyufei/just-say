param(
    [ValidateSet("build", "run", "install", "clean", "package")]
    [string]$Task = "build",
    [string]$InstallDir = "$env:LOCALAPPDATA\VoiceTray"
)

$ErrorActionPreference = "Stop"
$Target = "x86_64-pc-windows-msvc"

switch ($Task) {
    "build" {
        cargo build --release --target $Target
    }
    "run" {
        cargo run --release --target $Target
    }
    "install" {
        cargo build --release --target $Target
        New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
        Copy-Item "target\$Target\release\voicetray.exe" "$InstallDir\voicetray.exe" -Force
        Write-Host "Installed to $InstallDir\voicetray.exe"
    }
    "clean" {
        cargo clean
    }
    "package" {
        cargo build --release --target $Target
        $Out = "dist"
        New-Item -ItemType Directory -Force -Path $Out | Out-Null
        Copy-Item "target\$Target\release\voicetray.exe" "$Out\voicetray.exe" -Force
        Compress-Archive -Path "$Out\voicetray.exe", "README.md" -DestinationPath "$Out\VoiceTray.zip" -Force
        Write-Host "Package: $Out\VoiceTray.zip"
    }
}
