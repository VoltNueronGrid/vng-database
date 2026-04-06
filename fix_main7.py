"""Remove stale duplicate audit test section using correct string."""
path = r"D:\by\polap-db\services\voltnuerongridd\src\main.rs"
with open(path, "r", encoding="utf-8") as f:
    content = f.read()

# Use exact byte-level match - the issue is the ─ character encoding
idx_stale = content.find("// \u2500\u2500\u2500 S7-WS6-02: Raft consensus                voltnuerongrid_audit")
if idx_stale == -1:
    print("ANCHOR NOT FOUND")
    exit(1)

# Find the end: the next `// ─── S7-WS6-02: Raft consensus ─` (with dashes)
idx_real_raft = content.find("    // \u2500\u2500\u2500 S7-WS6-02: Raft consensus \u2500", idx_stale + 10)
if idx_real_raft == -1:
    print("REAL RAFT SECTION NOT FOUND")
    exit(1)

print(f"Removing chars {idx_stale - 6} to {idx_real_raft} ({idx_real_raft - idx_stale + 6} chars)")
# Remove from just before the stale comment (including the \n\n prefix) to the real raft start
# The stale section starts 6 chars back (for the "    // " portion)
stale_start = idx_stale - 6  # back to include "    "
old_stale = content[stale_start : idx_real_raft]
print(f"First 80: {repr(old_stale[:80])}")
print(f"Last  80: {repr(old_stale[-80:])}")

# Replace with just a newline
content = content[:stale_start] + "\n" + content[idx_real_raft:]
with open(path, "w", encoding="utf-8") as f:
    f.write(content)
print("Done")
