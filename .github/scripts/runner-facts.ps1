<#
.SYNOPSIS
The runner facts of W11, gathered on a Windows CI leg as the job's user. The runner is
disposable: this drops the probe's throwaway marker, makes and mounts two small VHDs, makes a
throwaway standard user, turns Developer Mode on and grants that user the symbolic-link right
for one run each, and undoes each of those again.

.DESCRIPTION
Everything goes into one JSON report at -Out. A fact that cannot be gathered is recorded as
an error in the report and does not fail the step; the step fails only when the probe's own
report is missing, which is a bug in the probe. Nothing a program prints to standard output
is parsed but the probe's reports, so children's output cannot break the report. No SID and
no user name is written into it.
#>
param(
    [Parameter(Mandatory)] [string] $Probe,
    [Parameter(Mandatory)] [string] $Triple,
    [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = 'Stop'

$Probe = (Resolve-Path -Path $Probe).Path
$scratch = Join-Path $env:RUNNER_TEMP 'pitboard-probe-scratch'
New-Item -ItemType Directory -Force -Path $scratch | Out-Null
# The probe looks for its marker in the folder FOLDERID_Profile names, which is what this
# returns.
$profileDir = [Environment]::GetFolderPath('UserProfile')
$marker = Join-Path $profileDir 'pitboard-probe-throwaway.marker'

function Read-Json([string] $Path) {
    if (-not (Test-Path -Path $Path)) { return $null }
    $text = Get-Content -Raw -Path $Path
    try { return ($text | ConvertFrom-Json) } catch { return [ordered]@{ unreadable_bytes = $text.Length } }
}

function Invoke-Fact([scriptblock] $Block) {
    try { return (& $Block) } catch { return [ordered]@{ error = $_.Exception.Message } }
}

# Run the probe with its report written to a file, and hand the report back.
function Invoke-Probe([string] $Label, [string[]] $ProbeArgs) {
    $file = Join-Path $scratch "report-$Label.json"
    Remove-Item -Force -Path $file -ErrorAction SilentlyContinue
    & $Probe --out $file @ProbeArgs *> $null
    [ordered]@{ exit = $LASTEXITCODE; report = (Read-Json $file) }
}

function Get-Policies {
    $ci = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\CI\Policy' -ErrorAction SilentlyContinue
    $nt = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
    $saferKey = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Safer\CodeIdentifiers'
    $devKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock'
    [ordered]@{
        os = [ordered]@{
            product = $nt.ProductName
            edition = $nt.EditionID
            installation_type = $nt.InstallationType
            display_version = $nt.DisplayVersion
            current_build = $nt.CurrentBuild
            ubr = $nt.UBR
        }
        smart_app_control_state = if ($ci) { $ci.VerifiedAndReputablePolicyState } else { $null }
        safer_default_level = if (Test-Path $saferKey) { (Get-ItemProperty -Path $saferKey).DefaultLevel } else { $null }
        safer_policy_present = Test-Path $saferKey
        applocker_effective_rules = Invoke-Fact {
            $p = Get-AppLockerPolicy -Effective
            ($p.RuleCollections | ForEach-Object { @($_).Count } | Measure-Object -Sum).Sum
        }
        wdac = Invoke-Fact {
            $g = Get-CimInstance -Namespace 'root\Microsoft\Windows\DeviceGuard' -ClassName 'Win32_DeviceGuard'
            [ordered]@{
                kernel_code_integrity = $g.CodeIntegrityPolicyEnforcementStatus
                user_mode_code_integrity = $g.UsermodeCodeIntegrityPolicyEnforcementStatus
            }
        }
        defender = Invoke-Fact {
            $m = Get-MpComputerStatus
            [ordered]@{
                service = $m.AMServiceEnabled
                antivirus = $m.AntivirusEnabled
                real_time = $m.RealTimeProtectionEnabled
                behavior_monitor = $m.BehaviorMonitorEnabled
                tamper_protected = $m.IsTamperProtected
            }
        }
        developer_mode = if (Test-Path $devKey) {
            (Get-ItemProperty -Path $devKey -ErrorAction SilentlyContinue).AllowDevelopmentWithoutDevLicense
        } else { $null }
        winget_present = $null -ne (Get-Command -Name 'winget' -ErrorAction SilentlyContinue)
        scoop_present = $null -ne (Get-Command -Name 'scoop' -ErrorAction SilentlyContinue)
    }
}

# W16's volume test: can the job make, format and mount an exFAT or FAT32 VHD with diskpart,
# and what do the rename routes do there.
function Get-Vhds {
    $dir = 'C:\pitboard-probe-vhd'
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $results = @()
    foreach ($fs in 'exfat', 'fat32') {
        $file = Join-Path $dir "pitboard-probe-$fs.vhdx"
        $letter = [char[]](68..90) | Where-Object { -not (Test-Path -Path "$($_):\") } | Select-Object -Last 1
        $create = Join-Path $dir "create-$fs.txt"
        @(
            "create vdisk file=`"$file`" maximum=256 type=expandable",
            "select vdisk file=`"$file`"",
            'attach vdisk',
            'create partition primary',
            "format fs=$fs quick label=PBPROBE",
            "assign letter=$letter"
        ) | Set-Content -Path $create -Encoding ascii
        $made = diskpart /s $create | Out-String
        $madeExit = $LASTEXITCODE
        $entry = [ordered]@{ fs = $fs; diskpart_exit = $madeExit; mounted = (Test-Path -Path "$($letter):\") }
        if ($entry.mounted) {
            $volumeScratch = "$($letter):\pitboard-probe"
            $entry.volume = Invoke-Probe "volume-$fs" @('volume', '--scratch', $volumeScratch)
            $entry.flush_dir = Invoke-Probe "flush-$fs" @('flush-dir', '--scratch', $volumeScratch)
            $entry.remove_home_while_locked = Invoke-Probe "lfx-$fs" @('lockfileex', '--mode', 'remove-home', '--seconds', '5', '--scratch', $volumeScratch)
        } else {
            $entry.diskpart_tail = ($made -split "`r?`n" | Where-Object { $_.Trim() } | Select-Object -Last 4) -join ' | '
        }
        $detach = Join-Path $dir "detach-$fs.txt"
        @("select vdisk file=`"$file`"", 'detach vdisk') | Set-Content -Path $detach -Encoding ascii
        diskpart /s $detach *> $null
        $entry.detach_exit = $LASTEXITCODE
        Remove-Item -Force -Path $file -ErrorAction SilentlyContinue
        $results += $entry
    }
    Remove-Item -Recurse -Force -Path $dir -ErrorAction SilentlyContinue
    $results
}

# W12 and W13's mechanism: can the job start a process as a fresh local standard user, and
# what can that user do. Each run's standard output goes to a file the job reads; a run that
# cannot reach the window station exits 0xC0000142.
function Get-FreshUser {
    $name = 'pbprobe' + (Get-Random -Minimum 10000 -Maximum 99999)
    $secure = ConvertTo-SecureString -String ([guid]::NewGuid().ToString('N') + 'Aa1!') -AsPlainText -Force
    $user = New-LocalUser -Name $name -Password $secure -AccountNeverExpires -PasswordNeverExpires
    Add-LocalGroupMember -Group 'Users' -Member $name -ErrorAction SilentlyContinue
    $sid = $user.SID.Value
    $cred = [pscredential]::new($name, $secure)
    $runDir = 'C:\pitboard-probe-runas'
    New-Item -ItemType Directory -Force -Path $runDir | Out-Null
    icacls $runDir /grant "${name}:(OI)(CI)M" *> $null
    $result = [ordered]@{}

    $runAs = {
        param([string] $Label, [string[]] $ProbeArgs)
        $stdout = Join-Path $scratch "runas-$Label-out.txt"
        $stderr = Join-Path $scratch "runas-$Label-err.txt"
        try {
            $p = Start-Process -FilePath $Probe -ArgumentList $ProbeArgs -Credential $cred `
                -WorkingDirectory $runDir -LoadUserProfile -PassThru -Wait `
                -RedirectStandardOutput $stdout -RedirectStandardError $stderr
            [ordered]@{
                started = $true
                exit = $p.ExitCode
                exit_hex = ('0x{0:X8}' -f $p.ExitCode)
                report = (Read-Json $stdout)
            }
        } catch {
            [ordered]@{ started = $false; error = $_.Exception.Message }
        }
    }

    try {
        $result.logon = & $runAs 'logon' @('logon')
        $result.unsigned_probe_ran = ($result.logon.started -and $result.logon.exit -eq 0)
        $userProfile = (Get-CimInstance -ClassName Win32_UserProfile | Where-Object { $_.SID -eq $sid }).LocalPath
        $result.profile_made = [bool] $userProfile
        if ($userProfile) {
            Set-Content -Path (Join-Path $userProfile 'pitboard-probe-throwaway.marker') -Value 'ci runner, disposable'
        }
        $result.tokens = & $runAs 'tokens' @('tokens')
        $result.symlink_as_is = & $runAs 'sym-as-is' @('symlink', '--scratch', (Join-Path $runDir 'as-is'))

        $devKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock'
        $devBefore = (Get-ItemProperty -Path $devKey -ErrorAction SilentlyContinue).AllowDevelopmentWithoutDevLicense
        New-Item -Path $devKey -Force -ErrorAction SilentlyContinue | Out-Null
        Set-ItemProperty -Path $devKey -Name 'AllowDevelopmentWithoutDevLicense' -Value 1 -Type DWord
        try {
            $result.symlink_developer_mode = & $runAs 'sym-devmode' @('symlink', '--scratch', (Join-Path $runDir 'devmode'))
        } finally {
            if ($null -eq $devBefore) {
                Remove-ItemProperty -Path $devKey -Name 'AllowDevelopmentWithoutDevLicense' -ErrorAction SilentlyContinue
            } else {
                Set-ItemProperty -Path $devKey -Name 'AllowDevelopmentWithoutDevLicense' -Value $devBefore -Type DWord
            }
        }

        $inf = Join-Path $scratch 'pitboard-probe-rights.inf'
        $original = Join-Path $scratch 'pitboard-probe-rights-original.inf'
        $db = Join-Path $scratch 'pitboard-probe-rights.sdb'
        secedit /export /cfg $original /areas USER_RIGHTS *> $null
        $lines = Get-Content -Path $original
        $granted = $false
        $lines = $lines | ForEach-Object {
            if ($_ -match '^SeCreateSymbolicLinkPrivilege\s*=') { $granted = $true; "$_,*$sid" } else { $_ }
        }
        if (-not $granted) {
            $lines = $lines | ForEach-Object {
                if ($_ -eq '[Privilege Rights]') { $_; "SeCreateSymbolicLinkPrivilege = *S-1-5-32-544,*$sid" } else { $_ }
            }
        }
        Set-Content -Path $inf -Value $lines -Encoding Unicode
        secedit /configure /db $db /cfg $inf /areas USER_RIGHTS *> $null
        $result.right_granted_exit = $LASTEXITCODE
        try {
            $result.symlink_right_granted = & $runAs 'sym-right' @('symlink', '--scratch', (Join-Path $runDir 'right'))
        } finally {
            secedit /configure /db $db /cfg $original /areas USER_RIGHTS *> $null
            Remove-Item -Force -Path $inf, $original, $db -ErrorAction SilentlyContinue
        }
    } finally {
        Remove-LocalUser -Name $name -ErrorAction SilentlyContinue
        Get-CimInstance -ClassName Win32_UserProfile | Where-Object { $_.SID -eq $sid } |
            Remove-CimInstance -ErrorAction SilentlyContinue
        Remove-Item -Recurse -Force -Path $runDir -ErrorAction SilentlyContinue
    }
    $result
}

$report = [ordered]@{ triple = $Triple }
Set-Content -Path $marker -Value 'ci runner, disposable'
try {
    $report.job_user = Invoke-Probe 'runner-facts' @('runner-facts', '--scratch', $scratch)
    $report.policies = Invoke-Fact { Get-Policies }
    $report.vhd = Invoke-Fact { Get-Vhds }
    $report.fresh_standard_user = Invoke-Fact { Get-FreshUser }
} finally {
    Remove-Item -Force -Path $marker -ErrorAction SilentlyContinue
    $report | ConvertTo-Json -Depth 64 | Set-Content -Path $Out -Encoding utf8
}
Get-Content -Raw -Path $Out
if ($null -eq $report.job_user.report) {
    throw 'the probe wrote no runner-facts report'
}
exit 0
