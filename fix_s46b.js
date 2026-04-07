#!/usr/bin/env node
// Session 46b — fix handler and test errors in main.rs
'use strict';
const fs = require('fs');
const path = require('path');

const filePath = path.join(__dirname, 'services', 'voltnuerongridd', 'src', 'main.rs');
let content = fs.readFileSync(filePath, 'utf8');

// Fix 1: require_admin_key -> require_operator_auth in wal_age handler
content = content.replace(
  `/// S11-WS1-22: Return the oldest and newest WAL sequence numbers and their span.
async fn wal_age(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalAgeResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_key(&headers, &state)?;
    let wal = state.wal.lock().await;
    let records = wal.wal_records();`,
  `/// S11-WS1-22: Return the oldest and newest WAL sequence numbers and their span.
async fn wal_age(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalAgeResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_age");
    let records = wal.wal_records();`
);
console.log('Fixed wal_age handler auth + lock');

// Fix 2: require_admin_key -> require_operator_auth in rows_first_key handler
//        state.row_store.lock().await -> .lock().expect(...)
//        .into_keys().collect() -> .into_iter().map(|(k, _)| k).collect()
content = content.replace(
  `/// S11-WS1-22: Return the first (alphabetically smallest) key currently in the row store.
async fn rows_first_key(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsFirstKeyResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_key(&headers, &state)?;
    let rs = state.row_store.lock().await;
    let mut keys: Vec<String> = rs.export_rows_snapshot().into_keys().collect();`,
  `/// S11-WS1-22: Return the first (alphabetically smallest) key currently in the row store.
async fn rows_first_key(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsFirstKeyResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_first_key");
    let mut keys: Vec<String> = rs.export_rows_snapshot().into_iter().map(|(k, _)| k).collect();`
);
console.log('Fixed rows_first_key handler auth + lock + keys collection');

// Fix 3: admin_headers -> operator_headers in tests
content = content.replace(
  `    async fn s11_ws1_22_wal_age_returns_ok_with_span() {
        let state = state_with_key(Some("test-key"));
        let headers = admin_headers("test-key");`,
  `    async fn s11_ws1_22_wal_age_returns_ok_with_span() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");`
);
content = content.replace(
  `    async fn s11_ws1_22_rows_first_key_returns_ok_empty_store() {
        let state = state_with_key(Some("test-key"));
        let headers = admin_headers("test-key");`,
  `    async fn s11_ws1_22_rows_first_key_returns_ok_empty_store() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");`
);
console.log('Fixed test headers: admin_headers -> operator_headers');

fs.writeFileSync(filePath, content, 'utf8');
console.log('Done!');
