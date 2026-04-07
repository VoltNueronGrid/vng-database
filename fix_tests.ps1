$f = 'd:\by\polap-db\services\voltnuerongridd\src\main.rs'
$s = [IO.File]::ReadAllText($f)
$crlf = "`r`n"

# Fix s11_ws1_28_rows_field_count_ok test
$old = "    async fn s11_ws1_28_rows_field_count_ok() {" + $crlf +
'        let state = state_with_key(Some("test-key"));' + $crlf +
'        let hdrs = operator_headers("test-key", "admin");' + $crlf +
"        let res = rows_field_count(State(state), hdrs).await;" + $crlf +
'        assert!(res.is_ok(), "rows_field_count should return ok");' + $crlf +
"        let body = res.unwrap().0;" + $crlf +
'        assert_eq!(body.status, "ok");' + $crlf +
"    }"
$new = "    async fn s11_ws1_28_rows_field_count_ok() {" + $crlf +
'        let state = state_with_key(Some("test-key"));' + $crlf +
'        let hdrs = operator_headers("test-key", "admin");' + $crlf +
'        let (status, Json(body)) = rows_field_count(State(state), hdrs).await.unwrap();' + $crlf +
"        assert_eq!(status, StatusCode::OK);" + $crlf +
'        assert_eq!(body.status, "ok");' + $crlf +
"    }"
if ($s.Contains($old)) { $s = $s.Replace($old, $new); Write-Host "TEST1 OK" } else { Write-Host "TEST1 FAIL"; exit 1 }

# Fix s11_ws1_28_wal_entry_latest_ok test
$old2 = "    async fn s11_ws1_28_wal_entry_latest_ok() {" + $crlf +
'        let state = state_with_key(Some("test-key"));' + $crlf +
'        let hdrs = operator_headers("test-key", "admin");' + $crlf +
"        let res = wal_entry_latest(State(state), hdrs).await;" + $crlf +
'        assert!(res.is_ok(), "wal_entry_latest should return ok");' + $crlf +
"        let body = res.unwrap().0;" + $crlf +
'        assert_eq!(body.status, "ok");' + $crlf +
"    }"
$new2 = "    async fn s11_ws1_28_wal_entry_latest_ok() {" + $crlf +
'        let state = state_with_key(Some("test-key"));' + $crlf +
'        let hdrs = operator_headers("test-key", "admin");' + $crlf +
'        let (status, Json(body)) = wal_entry_latest(State(state), hdrs).await.unwrap();' + $crlf +
"        assert_eq!(status, StatusCode::OK);" + $crlf +
'        assert_eq!(body.status, "ok");' + $crlf +
"    }"
if ($s.Contains($old2)) { $s = $s.Replace($old2, $new2); Write-Host "TEST2 OK" } else { Write-Host "TEST2 FAIL"; exit 1 }

[IO.File]::WriteAllText($f, $s)
Write-Host "Done"
