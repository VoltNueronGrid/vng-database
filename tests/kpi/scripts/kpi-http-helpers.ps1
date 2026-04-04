# Shared JSON HTTP helper for KPI smoke scripts.
# PowerShell 7+ Invoke-WebRequest throws HttpResponseException with HttpResponseMessage (no GetResponseStream).
# Windows PowerShell 5.x may surface HttpWebResponse via WebException (stream-based body).

function Invoke-HttpJson {
  param(
    [string]$Method,
    [string]$Uri,
    [hashtable]$Headers = $null,
    [object]$Body = $null,
    [int]$TimeoutSec = 20,
    [int]$JsonDepth = 8
  )

  $requestParams = @{
    Method = $Method
    Uri = $Uri
    TimeoutSec = $TimeoutSec
    UseBasicParsing = $true
  }
  if ($Headers) { $requestParams.Headers = $Headers }
  if ($null -ne $Body) {
    $requestParams.Body = ($Body | ConvertTo-Json -Depth $JsonDepth)
    $requestParams.ContentType = "application/json"
  }

  try {
    $response = Invoke-WebRequest @requestParams
    $json = $null
    if ($response.Content) {
      try { $json = $response.Content | ConvertFrom-Json } catch { $json = $null }
    }
    return [pscustomobject]@{
      StatusCode = [int]$response.StatusCode
      Json = $json
      Content = $response.Content
    }
  } catch {
    $statusCode = 0
    $content = ""

    $ex = $_.Exception
    while ($null -ne $ex) {
      $resp = $ex.Response
      if ($null -ne $resp) {
        if ($resp -is [System.Net.Http.HttpResponseMessage]) {
          try { $statusCode = [int]$resp.StatusCode } catch {}
          try {
            if ($null -ne $resp.Content) {
              $content = $resp.Content.ReadAsStringAsync().GetAwaiter().GetResult()
            }
          } catch {}
          break
        }
        if ($resp -is [System.Net.HttpWebResponse]) {
          try { $statusCode = [int]$resp.StatusCode } catch {}
          try {
            $stream = $resp.GetResponseStream()
            if ($null -ne $stream) {
              $reader = New-Object System.IO.StreamReader($stream)
              $content = $reader.ReadToEnd()
              $reader.Dispose()
            }
          } catch {}
          break
        }
      }
      $ex = $ex.InnerException
    }

    if ([string]::IsNullOrWhiteSpace($content) -and $_.ErrorDetails -and $_.ErrorDetails.Message) {
      $content = $_.ErrorDetails.Message
    }

    $json = $null
    if ($content) {
      try { $json = $content | ConvertFrom-Json } catch { $json = $null }
    }
    return [pscustomobject]@{
      StatusCode = $statusCode
      Json = $json
      Content = $content
    }
  }
}
