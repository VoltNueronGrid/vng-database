param(
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [string]$AdminKey = ""
)

$ErrorActionPreference = "Stop"

function Invoke-VngJson {
  param(
    [string]$Method,
    [string]$Path,
    [hashtable]$Headers,
    [object]$Body
  )

  $uri = "$BaseUrl$Path"
  $bodyJson = if ($null -ne $Body) { $Body | ConvertTo-Json -Depth 10 } else { $null }

  try {
    return Invoke-WebRequest -Method $Method -Uri $uri -Headers $Headers -Body $bodyJson -ContentType "application/json"
  } catch {
    if ($_.Exception.Response) {
      return $_.Exception.Response
    }
    throw
  }
}

$headers = @{}
if ($AdminKey) {
  $headers["x-vng-admin-key"] = $AdminKey
}

$checks = @()

$health = Invoke-VngJson -Method "GET" -Path "/health" -Headers @{} -Body $null
$checks += [pscustomobject]@{ endpoint = "/health"; status = [int]$health.StatusCode; ok = ([int]$health.StatusCode -eq 200) }

$sql = Invoke-VngJson -Method "POST" -Path "/api/v1/sql/execute" -Headers $headers -Body @{ sql_batch = @("SELECT 1;"); request_id = "ide-smoke" }
$checks += [pscustomobject]@{ endpoint = "/api/v1/sql/execute"; status = [int]$sql.StatusCode; ok = ([int]$sql.StatusCode -in @(200,401,403)) }

$schema = Invoke-VngJson -Method "GET" -Path "/api/v1/ingest/schema/registry" -Headers $headers -Body $null
$checks += [pscustomobject]@{ endpoint = "/api/v1/ingest/schema/registry"; status = [int]$schema.StatusCode; ok = ([int]$schema.StatusCode -in @(200,401,403)) }

$failed = @($checks | Where-Object { -not $_.ok })
$checks | Format-Table -AutoSize

if ($failed.Count -gt 0) {
  Write-Error "IDE extension smoke failed."
  exit 1
}

Write-Host "IDE extension smoke passed."
