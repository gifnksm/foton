param(
    [Parameter(Mandatory = $true)]
    [string] $BootstrapConfigPath,
    [Parameter(Mandatory = $true)]
    [string] $TranscriptPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

New-Item -ItemType Directory -Force -Path (Split-Path -Path $TranscriptPath -Parent) | Out-Null
Start-Transcript -LiteralPath $TranscriptPath -Force | Out-Null

function Get-BootstrapConfig
{
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf))
    {
        throw ('bootstrap config file not found: {0}' -f $Path)
    }

    $raw = Get-Content -LiteralPath $Path -Raw
    if ([string]::IsNullOrWhiteSpace($raw))
    {
        throw ('bootstrap config file is empty: {0}' -f $Path)
    }

    $raw | ConvertFrom-Json
}

function Set-EnvironmentVariable
{
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,
        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string] $Value
    )

    [System.Environment]::SetEnvironmentVariable($Name, $Value, [System.EnvironmentVariableTarget]::Machine)
    Set-Item -LiteralPath ('Env:{0}' -f $Name) -Value $Value
}

function Set-ConfiguredEnvironmentVariables
{
    param(
        [Parameter(Mandatory = $true)]
        [object] $Config
    )

    if ($null -eq $Config.environments)
    {
        throw 'bootstrap config does not contain environments'
    }

    $environmentProperties = $Config.environments.PSObject.Properties | Sort-Object -Property Name
    foreach ($property in $environmentProperties)
    {
        $name = [string] $property.Name
        $value = [string] $property.Value
        Set-EnvironmentVariable -Name $name -Value $value
    }
}

function Invoke-ScenarioRun
{
    $scenario = [System.Environment]::GetEnvironmentVariable('FOTON_SCENARIO', [System.EnvironmentVariableTarget]::Process)
    if ([string]::IsNullOrWhiteSpace($scenario))
    {
        return 0
    }

    $xtaskExe = [System.Environment]::GetEnvironmentVariable('FOTON_XTASK_EXE', [System.EnvironmentVariableTarget]::Process)
    if ([string]::IsNullOrWhiteSpace($xtaskExe))
    {
        throw 'FOTON_XTASK_EXE is missing after environment setup'
    }

    & $xtaskExe scenario run
    return $LASTEXITCODE
}

try
{
    $config = Get-BootstrapConfig -Path $BootstrapConfigPath
    Set-ConfiguredEnvironmentVariables -Config $config
    $exitCode = Invoke-ScenarioRun
    exit $exitCode
} finally
{
    Stop-Transcript | Out-Null
}
