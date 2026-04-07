#!/usr/bin/env node
// Fix: add has_interval field to SelectStatement struct in ast.rs
'use strict';
const fs = require('fs');
const file = 'd:/by/polap-db/crates/voltnuerongrid-sql/src/ast.rs';
let c = fs.readFileSync(file, 'utf8');

// Find exact content around has_trim
const idx = c.indexOf('pub has_trim: bool,');
console.log('has_trim at index:', idx);
console.log('Context:', JSON.stringify(c.substring(idx, idx + 120)));

// The struct closes with `}` right after has_trim line
// Replace `pub has_trim: bool,\n}` with field + interval field + `}`
const old1 = '    pub has_trim: bool,\n}';
const new1 = '    pub has_trim: bool,\n    /// True when the query uses an INTERVAL expression (date arithmetic) (S3-WS1-23).\n    pub has_interval: bool,\n}';

if (c.includes(old1)) {
  c = c.replace(old1, new1);
  console.log('Replaced successfully');
} else {
  // Try with different line endings
  const old2 = '    pub has_trim: bool,\r\n}';
  if (c.includes(old2)) {
    c = c.replace(old2, '    pub has_trim: bool,\r\n    /// True when the query uses an INTERVAL expression (date arithmetic) (S3-WS1-23).\r\n    pub has_interval: bool,\r\n}');
    console.log('Replaced with CRLF');
  } else {
    console.log('FAILED - trying to find the struct end');
    // Find where the SelectStatement struct ends
    const selectIdx = c.indexOf('pub struct SelectStatement');
    const afterSelect = c.indexOf('\n}', selectIdx);
    console.log('SelectStatement struct close at char:', afterSelect);
    console.log('Around close:', JSON.stringify(c.substring(afterSelect - 50, afterSelect + 10)));
  }
}

fs.writeFileSync(file, c, 'utf8');
console.log('has_interval present:', c.includes('pub has_interval'));
