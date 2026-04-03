$workdir = "d:\by\polap-db\services\voltnuerongridd"
Set-Location $workdir

Write-Host "=== Building test binary ===" -ForegroundColor Green
$buildOutput = cargo test --no-run 2>&1
Write-Host $buildOutput | Select-Object -Last 10

Write-Host ""
Write-Host "=== Checking test binary exists ===" -ForegroundColor Green
$testBinary = Get-ChildItem -Path "target/debug/deps" -Name "*voltnuerongridd*" -Type f | Where-Object { $_ -notmatch "\.d$" -and $_ -notmatch "\.rlib$" }
if ($testBinary) {
    Write-Host "Test binary found: $testBinary"
} else {
    Write-Host "Looking for test artifacts..." 
    Get-ChildItem -Path "target/debug/deps" -Name "*voltnuerongridd*" | Select-Object -First 20
}

Write-Host ""
Write-Host "=== Verifying test functions in source ===" -ForegroundColor Green
$testCount = (Select-String -Path "src/main.rs" -Pattern "fn ws3_" | Measure-Object).Count
Write-Host "Found $testCount WS3 test functions"

$testFunctions = Select-String -Path "src/main.rs" -Pattern "fn ws3_.*\(\)" | Select-Object -ExpandProperty Line
$testFunctions | ForEach-Object {
    Write-Host "  - $_"
}

Write-Host ""
Write-Host "=== Compiling and listing tests ===" -ForegroundColor Green
$testList = cargo test --lib 2>&1 | Select-String -Pattern "ws3_" | Select-Object -First 20
if ($testList) {
    Write-Host "Test list:"
    Write-Host $testList
} else {
    Write-Host "Running cargo test list command..."
    cargo test -- --list 2>&1 | Select-String "ws3_"
}
