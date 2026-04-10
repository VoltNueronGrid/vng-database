---
name: multi-agent-delivery
description: "Parallel multi-agent execution workflow. Use when: tasks are multi-step, need faster discovery/validation, or require simultaneous analysis across code, tests, and trackers."
argument-hint: "Goal, e.g. 'implement + validate WS6 change quickly'"
---
# Multi-Agent Delivery Skill

## Standard Parallel Pattern
- Agent 1: codebase discovery and impact map.
- Agent 2: risk and security review.
- Agent 3: testing and validation plan.
- Optional Agent 4: tracker/documentation sync.

## Rules
- Parallelize read-only work first.
- Merge findings into a single implementation plan.
- Execute edits centrally, then validate with tests.
- Report deltas only; avoid duplicate findings.

## Completion Criteria
- All relevant checks pass.
- Tracker/docs updated if scope requires it.
- No unresolved high/critical findings.
