#!/usr/bin/env node
// Fix planner.rs: add Interval variant + all arms + plan_select update
// Handles CRLF line endings in the file
'use strict';
const fs = require('fs');
const file = 'd:/by/polap-db/crates/voltnuerongrid-exec/src/planner.rs';
let c = fs.readFileSync(file, 'utf8');

// Detect line ending
const hasCRLF = c.includes('\r\n');
const NL = hasCRLF ? '\r\n' : '\n';
console.log('Line ending:', hasCRLF ? 'CRLF' : 'LF');

// Helper: replace with any line ending
function flexReplace(old, replacement) {
  // Try both LF and CRLF variants
  const oldLF = old.replace(/\r\n/g, '\n').replace(/\n/g, '\n');
  const oldCRLF = old.replace(/\r\n/g, '\n').replace(/\n/g, '\r\n');
  const repLF = replacement.replace(/\r\n/g, '\n').replace(/\n/g, '\n');
  
  if (c.includes(oldCRLF)) {
    const repCRLF = repLF.replace(/\n/g, '\r\n');
    c = c.replace(oldCRLF, repCRLF);
    console.log('Replaced (CRLF)');
    return true;
  } else if (c.includes(oldLF)) {
    c = c.replace(oldLF, repLF);
    console.log('Replaced (LF)');
    return true;
  }
  console.log('FAILED to replace');
  return false;
}

// 1. Add Interval variant after Trim
flexReplace(
  `    /// TRIM / LTRIM / RTRIM string function applied to result set (S3-WS1-22 has_trim support).
    Trim {
        input: Box<LogicalPlan>,
    },`,
  `    /// TRIM / LTRIM / RTRIM string function applied to result set (S3-WS1-22 has_trim support).
    Trim {
        input: Box<LogicalPlan>,
    },
    /// INTERVAL date arithmetic expression (S3-WS1-23 has_interval support).
    Interval {
        input: Box<LogicalPlan>,
    },`
);

// 2. Add Interval arm in primary_table()
flexReplace(
  `            LogicalPlan::Trim { input } => input.primary_table(),
            LogicalPlan::WindowFn { input, .. } => input.primary_table(),`,
  `            LogicalPlan::Trim { input } => input.primary_table(),
            LogicalPlan::Interval { input } => input.primary_table(),
            LogicalPlan::WindowFn { input, .. } => input.primary_table(),`
);

// 3. Add Interval arm in has_aggregation()
flexReplace(
  `            LogicalPlan::Trim { input } => input.has_aggregation(),
            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),`,
  `            LogicalPlan::Trim { input } => input.has_aggregation(),
            LogicalPlan::Interval { input } => input.has_aggregation(),
            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),`
);

// 4. Add Interval arm in estimate_cost()
flexReplace(
  `            LogicalPlan::Trim { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.05,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::WindowFn { input, .. } => {`,
  `            LogicalPlan::Trim { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.05,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Interval { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.9) as u64,
                    relative_cost: inner.relative_cost + 0.3,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::WindowFn { input, .. } => {`
);

// 5. Convert Trim bare if/else to let + add Interval outermost
flexReplace(
  `        // Trim wrapper (S3-WS1-22 has_trim detection): outermost node.
        if sel.has_trim {
            LogicalPlan::Trim {
                input: Box::new(after_not_in),
            }
        } else {
            after_not_in
        }
    }`,
  `        // Trim wrapper (S3-WS1-22 has_trim detection).
        let after_trim = if sel.has_trim {
            LogicalPlan::Trim {
                input: Box::new(after_not_in),
            }
        } else {
            after_not_in
        };

        // Interval wrapper (S3-WS1-23 has_interval detection): outermost node.
        if sel.has_interval {
            LogicalPlan::Interval {
                input: Box::new(after_trim),
            }
        } else {
            after_trim
        }
    }`
);

// Save
fs.writeFileSync(file, c, 'utf8');

// Verify
const hasVariant = c.includes('Interval {');
const hasCtArm = c.includes('Interval { input } => input.primary_table()');
const hasCostArm = c.includes('LogicalPlan::Interval { input } => {');
const hasPlanSelect = c.includes('after_trim');
console.log('Interval variant:', hasVariant);
console.log('primary_table arm:', hasCtArm);
console.log('cost arm:', hasCostArm);
console.log('plan_select converted:', hasPlanSelect);
