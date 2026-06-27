param(
    [ValidateSet("build", "run", "install", "clean", "package")]
    [string]$Task = "build",
    [string]$InstallDir = "$env:LOCALAPPDATA\JustSay"
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
        Copy-Item "target\$Target\release\justsay.exe" "$InstallDir\justsay.exe" -Force
        Write-Host "Installed to $InstallDir\justsay.exe"
    }
    "clean" {
        cargo clean
    }
    "package" {
        cargo build --release --target $Target
        $Out = "dist"
        New-Item -ItemType Directory -Force -Path $Out | Out-Null
        Copy-Item "target\$Target\release\justsay.exe" "$Out\justsay.exe" -Force
        Compress-Archive -Path "$Out\justsay.exe", "README.md" -DestinationPath "$Out\JustSay.zip" -Force
        Write-Host "Package: $Out\JustSay.zip"
    }
}
