$ErrorActionPreference = 'Stop'
$setup = $env:WINX_UPDATE_SETUP
$work = Split-Path -LiteralPath $setup
$launch = $env:WINX_UPDATE_CURRENT
$expected = $env:WINX_UPDATE_VERSION
$verified = $false
$failure = $null
$lock = $null
try {
    $old = Get-Process -Id ([int]$env:WINX_UPDATE_OLD_PID) -ErrorAction SilentlyContinue
    if ($old -and !$old.WaitForExit(30000)) {
        throw 'WinX did not close in time. The installer was not started.'
    }
    $lock = [IO.File]::Open($setup, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $hash = (Get-FileHash -InputStream $lock -Algorithm SHA256).Hash
    if ($hash -ne $env:WINX_UPDATE_DIGEST) {
        throw 'The saved installer changed after verification. Download it again from Updates.'
    }
    # Interactive NSIS setup lets the user cancel and shows Windows publisher prompts.
    $installer = Start-Process -FilePath $setup -PassThru -Wait
    if ($installer.ExitCode -eq 2) { throw 'Installation was cancelled.' }
    if ($installer.ExitCode -ne 0) { throw "The installer exited with code $($installer.ExitCode)." }
    $key = Get-ItemProperty -LiteralPath 'HKCU:\Software\NothingXDesktop' -Name InstallDir
    $installed = Join-Path $key.InstallDir 'nothingx-desktop.exe'
    if (!(Test-Path -LiteralPath $installed -PathType Leaf)) {
        throw 'The installed application could not be found.'
    }
    if ([Diagnostics.FileVersionInfo]::GetVersionInfo($installed).ProductVersion -cne $expected) {
        throw "The installed WinX version did not match $expected. Run the verified installer again."
    }
    $launch = $installed
    $verified = $true
} catch {
    $failure = $_.Exception.Message
} finally {
    if ($lock) { $lock.Dispose() }
    Remove-Item -LiteralPath $setup -ErrorAction SilentlyContinue
}
try {
    if ($verified) {
        $run = Split-Path -Leaf $work
        $ready = Join-Path $work 'started'
        $new = Start-Process -FilePath $launch -ArgumentList @('--open', '--update-ack', $run) -PassThru
        $deadline = [DateTime]::UtcNow.AddSeconds(25)
        while (!(Test-Path -LiteralPath $ready -PathType Leaf)) {
            if ($new.HasExited -or [DateTime]::UtcNow -ge $deadline) {
                throw "WinX $expected is installed but could not confirm startup. Open WinX from Start."
            }
            Start-Sleep -Milliseconds 250
        }
        if ((Get-Content -LiteralPath $ready -Raw) -cne $expected) {
            throw 'The relaunched WinX reported an unexpected version.'
        }
    } elseif ((!$old -or $old.HasExited) -and (Test-Path -LiteralPath $launch -PathType Leaf)) {
        Start-Process -FilePath $launch -ArgumentList '--open'
    }
} catch {
    $failure = (@($failure, $_.Exception.Message) | Where-Object { $_ }) -join ' '
} finally {
    Remove-Item -LiteralPath (Join-Path $work 'started') -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $work -ErrorAction SilentlyContinue
}
if ($failure) {
    Add-Type -AssemblyName System.Windows.Forms
    [void][Windows.Forms.MessageBox]::Show($failure, 'WinX update')
}
