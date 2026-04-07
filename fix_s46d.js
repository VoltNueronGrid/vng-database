#!/usr/bin/env node
// Session 46d — comprehensive fix:
//  1. Remove S22 tests from beginning (mis-inserted)
//  2. Restore fn-closing } for s11_ws1_21
//  3. Insert S22 tests properly before mod tests closing }
'use strict';
const fs = require('fs');
const path = require('path');

const filePath = path.join(__dirname, 'services', 'voltnuerongridd', 'src', 'main.rs');
let lines = fs.readFileSync(filePath, 'utf8').split('\n');

console.log('Start total lines:', lines.length);

// ── Step 1: Remove the S22 block from the BEGINNING of the file
// It starts at index 0 (empty line) and goes until the last S22 test's closing }
// then the fn-close `    }` that was incorrectly included

// Find all indices of s11_ws1_22 test declarations near the top
let topS22Start = -1;
let topS22End = -1;

// The block starts at line 1 (index 0) and is the empty line before the comment
// Let's find the first reference to "S11-WS1-22: WAL age + rows first key tests"
for (let i = 0; i < 50; i++) {
  if (lines[i] && lines[i].includes('S11-WS1-22: WAL age + rows first key tests')) {
    // Block starts at i-1 (the empty line before comment)
    topS22Start = Math.max(0, i - 1);
    break;
  }
}
console.log('Top S22 block starts at index:', topS22Start);

// Find the end of the top S22 block:
// It ends after the last s11_ws1_22 test closing } and its trailing empty line
// and the fn-closing `    }` that was inserted
for (let i = topS22Start + 1; i < 50; i++) {
  const stripped = lines[i] ? lines[i].replace(/\r$/, '') : '';
  if (stripped === '    }') {
    // This is the fn-close that got mis-included
    topS22End = i;
    // Keep going to check for one more trailing empty
    if (lines[i + 1] !== undefined && lines[i + 1].replace(/\r$/, '') === '') {
      topS22End = i + 1;
    }
    break;
  }
}
// If we didn't find `    }`, find the last matching line up to position 45
if (topS22End === -1) {
  for (let i = 44; i >= topS22Start; i--) {
    if (lines[i] && lines[i].replace(/\r$/, '').trim() === '}') {
      topS22End = i;
      break;
    }
  }
}
console.log('Top S22 block ends at index:', topS22End);

// Extract the S22 test lines (exclude the fn-close `    }` if it's there)
const topBlock = lines.slice(topS22Start, topS22End + 1);
console.log('Top block extracted (', topBlock.length, 'lines):');
topBlock.forEach((l, i) => console.log('  [' + i + ']', JSON.stringify(l)));

// Remove the block from the top
lines.splice(topS22Start, topS22End - topS22Start + 1);
console.log('After removal, total lines:', lines.length);

// ── Step 2: Verify/fix the end of the file —
// The last lines should be:
//   ...s11_ws1_21 fn body...
//   }    <- fn close (missing!)
//   }    <- mod close

// Find the mod closing } (last unindented } in the file)
let modCloseIdx = -1;
for (let i = lines.length - 1; i >= 0; i--) {
  const stripped = lines[i].replace(/\r$/, '');
  if (stripped === '}') { modCloseIdx = i; break; }
}
console.log('mod close at index:', modCloseIdx, '(line', modCloseIdx + 1, ')');

// Check what's just before the mod close
console.log('\nLast 8 lines:');
for (let i = lines.length - 8; i < lines.length; i++) {
  if (lines[i] !== undefined) console.log(i + 1, JSON.stringify(lines[i]));
}

// The s11_ws1_21 function is not closed (missing `    }`)
// Insert `    }` before the blank line before the mod close
// Structure to achieve:
//   ...
//   assert_eq!(...);   <- last line of s11_ws1_21 body
//   }                  <- close s11_ws1_21 function (we insert this)
//   ... S22 tests ...
//   }                  <- close mod tests (already exists)

// Find the exact position to insert:
// After the assert_eq line of s11_ws1_21, before the trailing blank line(s) and mod close
// The assert_eq line is modCloseIdx - 2 (currently: assert_eq, \r, })
// But let's just insert before the blank line

// Insert fn-close `    }` and the S22 tests before the mod close
// The S22 tests we want to add (extracted from top, excluding the fn-close at the end)
const fnCloseBlock = topBlock.filter((_, i) => {
  const stripped = topBlock[i].replace(/\r$/, '');
  // Exclude trailing `    }` (that was the fn-close accidentally included)
  return true; // We'll handle this differently
});

// Actually, let's be explicit:
// We want to insert:
//   1. `    }` (fn close for s11_ws1_21)
//   2. Empty line
//   3. S22 tests block (from the top block, up to but NOT including the `    }`)
// The top block contains:
//   topBlock[0]: '' (empty before S22 comment)
//   topBlock[1]: '    // ── S11-WS1-22: ...'
//   topBlock[2]: '' (empty)
//   topBlock[3]: '    #[tokio::test]'
//   ...
//   topBlock[last-2 or last-1]: '    }' (last test fn close)
//   topBlock[last-1 or last]: '' (trailing empty or fn-close)

// Determine which part of topBlock is the actual S22 tests
// vs the accidentally-included fn-close
let lastS22TestEnd = -1;
for (let i = topBlock.length - 1; i >= 0; i--) {
  const stripped = topBlock[i].replace(/\r$/, '').trim();
  if (stripped === '}' && topBlock[i].replace(/\r$/, '').startsWith('    ')) {
    // Found a `    }` - check if it's the fn close or a test close
    // The fn-close was accidentally included as the LAST `    }` in the original s22LastBrace detection
    // It could be at the end of the top block
    // The actual last S22 test `}` is the one BEFORE any trailing empty at the very end
    if (i === topBlock.length - 1 || topBlock[i + 1].replace(/\r$/, '') === '') {
      // This might be the fn-close that was accidentally included, skip it
      if (i > 0 && topBlock[i - 1].replace(/\r$/, '') === '') {
        // The line before is empty, so this `    }` closes the last test  
        lastS22TestEnd = i;
        break;
      }
    } else {
      lastS22TestEnd = i;
      break;
    }
  }
}
console.log('\nlastS22TestEnd in topBlock:', lastS22TestEnd);

// Build the insertion block
// We'll check if the topBlock includes a `    }` that's actually the fn-close
// If the top block's last non-empty line is `    }` and the one before it is also `    }`
// then the last one is the fn-close
const insertBlock = ['    }', '']; // fn close + blank line

// Add the S22 test content (stopping before any accidentally-included fn-close)
// The topBlock starts with '' (empty), then the S22 comment, tests, etc.
// We include everything EXCEPT if the very last '}' that was the fn-close
// From the analysis, the topBlock had 43 lines including:
//   [0]: empty
//   [1]: S22 comment
//   [2]: empty
//   [3-11]: wal_age test
//   [12]: empty
//   [13-20]: wal_age_missing_auth test
//   [21]: empty
//   [22-32]: rows_first_key test
//   [33]: empty
//   [34-41]: rows_first_key_missing_auth test
//   [42]: empty <- trailing empty from testLines in fix_s46.js
// Then the fn-close `    }\r` was at topBlock[43] (but 43 means topBlock.length=44?)
// Let's just use the top block up to where all 4 test functions are complete

// Find last `    async fn s11_ws1_22` in topBlock
let lastS22FnStart = -1;
for (let i = topBlock.length - 1; i >= 0; i--) {
  if (topBlock[i].includes('async fn s11_ws1_22')) {
    lastS22FnStart = i;
    break;
  }
}
// Find where that function ends (next `    }`)
let lastS22FnClose = -1;
for (let i = lastS22FnStart + 1; i < topBlock.length; i++) {
  if (topBlock[i].replace(/\r$/, '') === '    }') {
    lastS22FnClose = i;
    break;
  }
}
console.log('Last S22 test fn at', lastS22FnStart, 'closes at', lastS22FnClose);

// Build clean S22 test block: from top block start, up to and including lastS22FnClose, plus empty line
const cleanS22Block = topBlock.slice(0, lastS22FnClose + 1);
cleanS22Block.push('');  // trailing empty line
console.log('Clean S22 block has', cleanS22Block.length, 'lines');

// Now insert before modCloseIdx:
// We insert: [fn-close, empty, ...S22tests]
const toInsert = [...insertBlock, ...cleanS22Block];
lines.splice(modCloseIdx, 0, ...toInsert);
console.log('Inserted', toInsert.length, 'lines before mod close');

console.log('\nFinal last 55 lines:');
for (let i = Math.max(0, lines.length - 55); i < lines.length; i++) {
  if (lines[i] !== undefined) console.log(i + 1, JSON.stringify(lines[i]));
}

fs.writeFileSync(filePath, lines.join('\n'), 'utf8');
console.log('\nDone! Total lines:', lines.length);
