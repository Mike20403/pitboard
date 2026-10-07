<#
.SYNOPSIS
VM block K1 only: trust, pack, sign, register and remove the self-signed sparse probe
package. Never run it outside the VM: the safety rules forbid registering a branch build or
this package anywhere else.

.DESCRIPTION
  -Step trust       elevated, once: make a self-signed code-signing certificate for
                    CN=PitboardProbe, export it to <WorkDir>, and trust it machine-wide in
                    Trusted People so a standard account can install the package.
  -Step register    as the account that will run the probe: pack the chosen manifest with a
                    1x1 logo, sign it, and register it with <ProbeDir> as its external
                    location, where pitboard-probe-sparse.exe must already be.
  -Step unregister  as that account: remove the package.
  -Step untrust     elevated: remove the certificate from both stores.

makeappx.exe and signtool.exe come with the Windows SDK, which this looks for under
"Windows Kits\10\bin".
#>
param(
    [Parameter(Mandatory)]
    [ValidateSet('trust', 'register', 'unregister', 'untrust')]
    [string] $Step,
    [ValidateSet('virtualized', 'unvirtualized')]
    [string] $Manifest = 'virtualized',
    [string] $ProbeDir = 'C:\probe',
    [string] $WorkDir = 'C:\probe\k1'
)
$ErrorActionPreference = 'Stop'
$subject = 'CN=PitboardProbe'
# A password for a self-signed certificate that exists only in this VM.
$pfxPassword = 'pitboard-probe-k1'

function Find-SdkTool([string] $name) {
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'arm64' } else { 'x64' }
    $all = @(Get-ChildItem -Path $kits -Filter $name -Recurse -ErrorAction SilentlyContinue |
            Sort-Object FullName -Descending)
    $native = $all | Where-Object { $_.Directory.Name -eq $arch } | Select-Object -First 1
    $any = $all | Select-Object -First 1
    $found = if ($native) { $native } else { $any }
    if (-not $found) { throw "$name is not under $kits; install the Windows SDK" }
    $found.FullName
}

switch ($Step) {
    'trust' {
        New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null
        $cert = New-SelfSignedCertificate -Type Custom -Subject $subject `
            -KeyUsage DigitalSignature -FriendlyName 'Pitboard Probe K1' `
            -CertStoreLocation 'Cert:\CurrentUser\My' `
            -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')
        $secure = ConvertTo-SecureString -String $pfxPassword -Force -AsPlainText
        Export-PfxCertificate -Cert $cert -FilePath (Join-Path $WorkDir 'probe.pfx') -Password $secure | Out-Null
        Export-Certificate -Cert $cert -FilePath (Join-Path $WorkDir 'probe.cer') | Out-Null
        Import-Certificate -FilePath (Join-Path $WorkDir 'probe.cer') `
            -CertStoreLocation 'Cert:\LocalMachine\TrustedPeople' | Out-Null
        "trusted CN=PitboardProbe"
    }
    'register' {
        $exe = Join-Path $ProbeDir 'pitboard-probe-sparse.exe'
        if (-not (Test-Path $exe)) { throw "$exe is missing" }
        $pkg = Join-Path $WorkDir 'pkg'
        Remove-Item -Recurse -Force -Path $pkg -ErrorAction SilentlyContinue
        New-Item -ItemType Directory -Force -Path (Join-Path $pkg 'Assets') | Out-Null
        $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'arm64' } else { 'x64' }
        $xml = Get-Content -Raw -Path (Join-Path $PSScriptRoot "AppxManifest-$Manifest.xml")
        $xml = $xml -replace 'ProcessorArchitecture="arm64"', "ProcessorArchitecture=`"$arch`""
        Set-Content -Path (Join-Path $pkg 'AppxManifest.xml') -Value $xml -Encoding utf8
        $png = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=='
        [IO.File]::WriteAllBytes((Join-Path $pkg 'Assets\probe.png'), [Convert]::FromBase64String($png))
        $msix = Join-Path $WorkDir "probe-$Manifest.msix"
        & (Find-SdkTool 'makeappx.exe') pack /d $pkg /p $msix /nv /o
        if ($LASTEXITCODE -ne 0) { throw "makeappx exited $LASTEXITCODE" }
        & (Find-SdkTool 'signtool.exe') sign /fd SHA256 /f (Join-Path $WorkDir 'probe.pfx') /p $pfxPassword $msix
        if ($LASTEXITCODE -ne 0) { throw "signtool exited $LASTEXITCODE" }
        Add-AppxPackage -Path $msix -ExternalLocation $ProbeDir
        Get-AppxPackage -Name 'PitboardProbe' | Select-Object Name, PackageFamilyName, Status
    }
    'unregister' {
        Get-AppxPackage -Name 'PitboardProbe' | Remove-AppxPackage
        "unregistered"
    }
    'untrust' {
        Get-ChildItem -Path 'Cert:\LocalMachine\TrustedPeople', 'Cert:\CurrentUser\My' |
            Where-Object { $_.Subject -eq $subject } | Remove-Item
        Remove-Item -Force -Path (Join-Path $WorkDir 'probe.pfx'), (Join-Path $WorkDir 'probe.cer') -ErrorAction SilentlyContinue
        "untrusted"
    }
}
