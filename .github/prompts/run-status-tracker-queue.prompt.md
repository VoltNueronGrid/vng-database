---
description: "Run the Status Tracker 10x Executor with only an iteration count input. Use when: continuing status-tracker work from the next pending step without providing a starting session."
name: "Run Status Tracker Queue"
argument-hint: "Number of iterations, e.g. 3"
agent: "Status Tracker 10x Executor"
---
Execute the status-tracker workflow for exactly the number of iterations provided in the prompt argument.

Requirements:
- Treat the prompt argument as the only user input and parse it as an integer iteration count.
- Do NOT ask for a starting session.
- Auto-discover the next pending step from `status_tracker.md` and `status-tracker-sprintwise-v1.md`.
- For each iteration: implement next slice, run targeted + full tests, update both tracker files, commit, and push.
- Stop immediately on the first failed iteration and report the blocker.

If the argument is missing or invalid:
- Ask for a valid positive integer and do not run the queue.
