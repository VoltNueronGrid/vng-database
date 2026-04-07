#!/usr/bin/env node
// Session 46c — fix test placement: move S22 tests outside s11_ws1_21 function body
'use strict';
const fs = require('fs');
const path = require('path');

const filePath = path.join(__dirname, 'services', 'voltnuerongridd', 'src', 'main.rs');
let lines = fs.readFileSync(filePath, 'utf8').split('\n');

console.log('Total lines:', lines.length);

// Find the s11_ws1_22 comment marker (marks start of inserted S22 tests)
let s22CommentIdx = -1;
for (let i = 0; i < lines.length; i++) {
  if (lines[i].includes('S11-WS1-22: WAL age + rows first key tests')) {
    s22CommentIdx = i;
    break;
  }
}
console.log('S22 comment at line:', s22CommentIdx + 1);

// The tests start 1 line before that (the empty line just before the comment)
// Actually the blank line at s22CommentIdx-1 is part of the insertion
// We want to find the end of the tests block (last `    }` before the final closing braces)
// Find the end of s22 tests (last `    }` before mod close)
let s22LastBrace = -1;
for (let i = lines.length - 1; i >= 0; i--) {
  const t = lines[i].replace(/\r$/, '').trim();
  if (t === '}' && lines[i].replace(/\r$/, '').startsWith('    ')) {
    // This is an indented }, find the last one before the unindented }
    s22LastBrace = i;
    break;
  }
}
console.log('S22 last test } at line:', s22LastBrace + 1);

// Find the mod tests closing } (last unindented } in the file)
let modCloseIdx = -1;
for (let i = lines.length - 1; i >= 0; i--) {
  if (lines[i].replace(/\r$/, '') === '}') {
    modCloseIdx = i;
    break;
  }
}
console.log('mod tests } at line:', modCloseIdx + 1);

// Between s22LastBrace+1 and modCloseIdx there should be exactly one `    }` (the fn closer)
// and possibly a blank line and the mod closer.
// Let's print lines from s22CommentIdx-2 to modCloseIdx+1 for context
console.log('\nContext:');
for (let i = s22CommentIdx - 2; i <= modCloseIdx + 1 && i < lines.length; i++) {
  console.log(i + 1, JSON.stringify(lines[i]));
}

// The structure we want:
// ... (last line of s11_ws1_21 body: assert_eq!)
//     }   <- close s11_ws1_21 function
//
//     // ── S11-WS1-22: WAL age + rows first key tests
//     ... S22 tests ...
//     }   <- close last S22 test
// }   <- close mod tests

// Extract the s22 test block (from the empty line before the comment to s22LastBrace)
const s22StartIdx = s22CommentIdx - 1; // the empty line before `// ── S11-WS1-22`
const s22TestBlock = lines.splice(s22StartIdx, s22LastBrace - s22StartIdx + 1);
console.log('\nExtracted', s22TestBlock.length, 'lines of S22 tests');

// After removal, find the function's closing `    }` which should now be right after the s11_ws1_21 body
// The s11_ws1_21 assert_eq line was at s22CommentIdx - 2 originally, now its body end is at s22StartIdx - 1
// The original line 22277 (0-indexed 22276) was `        assert_eq!(...)` and it had `\r`
// After the removal, the fn body closer `    }\r` should be visible

// Find it: look for `    }\r` after the end of the assert
let fnCloseIdx = -1;
for (let i = s22StartIdx; i < s22StartIdx + 5 && i < lines.length; i++) {
  console.log('After extraction line', i + 1, JSON.stringify(lines[i]));
  if (lines[i].replace(/\r$/, '') === '    }') {
    fnCloseIdx = i;
    break;
  }
}
console.log('fn close } at line:', fnCloseIdx + 1);

// Insert the s22 test block AFTER fnCloseIdx
lines.splice(fnCloseIdx + 1, 0, ...s22TestBlock);
console.log('Inserted S22 tests after fn closing }');

// Verify the final structure
const newModCloseIdx = lines.indexOf('}\r', fnCloseIdx);
const newModCloseIdx2 = lines.indexOf('}', fnCloseIdx);
console.log('\nFinal structure: last 15 lines:');
for (let i = lines.length - 15; i < lines.length; i++) {
  if(lines[i] !== undefined) console.log(i + 1, JSON.stringify(lines[i]));
}

fs.writeFileSync(filePath, lines.join('\n'), 'utf8');
console.log('\nDone! Total lines:', lines.length);
