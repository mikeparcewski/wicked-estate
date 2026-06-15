Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-SystemInfo {
    [CmdletBinding()]
    param(
        [string]$ComputerName = $env:COMPUTERNAME
    )
    [PSCustomObject]@{
        ComputerName = $ComputerName
        OS           = (Get-CimInstance Win32_OperatingSystem).Caption
        CPU          = (Get-CimInstance Win32_Processor).Name
        MemoryGB     = [math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB, 2)
    }
}

function Invoke-WithRetry {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][scriptblock]$ScriptBlock,
        [int]$MaxAttempts = 3,
        [int]$DelaySeconds = 2
    )
    $attempt = 0
    do {
        try {
            return & $ScriptBlock
        } catch {
            $attempt++
            if ($attempt -ge $MaxAttempts) { throw }
            Write-Warning "Attempt $attempt failed: $_. Retrying in ${DelaySeconds}s..."
            Start-Sleep -Seconds $DelaySeconds
        }
    } while ($true)
}

function Export-Report {
    [CmdletBinding()]
    param(
        [Parameter(ValueFromPipeline)][PSCustomObject]$InputObject,
        [Parameter(Mandatory)][string]$Path
    )
    begin   { $rows = @() }
    process { $rows += $InputObject }
    end     { $rows | Export-Csv -Path $Path -NoTypeInformation; Write-Host "Report written to $Path" }
}

Get-SystemInfo | Export-Report -Path "$env:TEMP\system-report.csv"
