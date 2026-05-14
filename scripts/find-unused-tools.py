#!/usr/bin/env python3
"""Find unused tools: no integration test, no AGENTS.md routing, not deprecated."""
import os, re, tomllib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

with open(os.path.join(ROOT, 'crates/codelens-mcp/tools.toml'), 'rb') as f:
    d = tomllib.load(f)
all_tools = set(t['name'] for t in d['tool'])

# Tools referenced in integration tests
tested_tools = set()
test_dir = os.path.join(ROOT, 'crates/codelens-mcp/src/integration_tests')
for root, dirs, files in os.walk(test_dir):
    for fname in files:
        if fname.endswith('.rs'):
            with open(os.path.join(root, fname)) as f:
                content = f.read()
            for tool in all_tools:
                if f'"{tool}"' in content:
                    tested_tools.add(tool)

# Tools referenced in AGENTS.md
agents_tools = set()
agents_path = os.path.join(ROOT, 'AGENTS.md')
if os.path.exists(agents_path):
    with open(agents_path) as f:
        content = f.read()
    for m in re.finditer(r'codelens__([a-z_]+)', content):
        if m.group(1) in all_tools:
            agents_tools.add(m.group(1))
    for m in re.finditer(r'`([a-z_]+)`', content):
        if m.group(1) in all_tools:
            agents_tools.add(m.group(1))

# Already deprecated
deprecated = {'audit_security_context', 'analyze_change_impact', 'assess_change_readiness', 'get_impact_analysis', 'find_dead_code'}

# Metadata.rs listed tools
meta_tools = set()
meta_path = os.path.join(ROOT, 'crates/codelens-mcp/src/tool_defs/presets/metadata.rs')
with open(meta_path) as f:
    meta = f.read()
for m in re.finditer(r'"([a-z_]+)"', meta):
    t = m.group(1)
    if t in all_tools:
        meta_tools.add(t)

# CLAUDE.md tool references (host routing)
claude_tools = set()
claude_path = os.path.join(ROOT, 'CLAUDE.md')
if os.path.exists(claude_path):
    with open(claude_path) as f:
        content = f.read()
    for m in re.finditer(r'codelens__([a-z_]+)', content):
        if m.group(1) in all_tools:
            claude_tools.add(m.group(1))
    for m in re.finditer(r'`([a-z_]+)`', content):
        if m.group(1) in all_tools:
            claude_tools.add(m.group(1))

# Unused = not in tests, AGENTS.md, CLAUDE.md, or deprecated
visible_tools = tested_tools | agents_tools | claude_tools | deprecated
unused = all_tools - visible_tools

print(f"Total tools: {len(all_tools)}")
print(f"Tested tools: {len(tested_tools)}")
print(f"AGENTS.md refs: {len(agents_tools)}")
print(f"CLAUDE.md refs: {len(claude_tools)}")
print(f"Metadata listed: {len(meta_tools)}")
print(f"Already deprecated: {len(deprecated)}")
print(f"\nUnused (no test, no AGENTS/CLAUDE.md, not deprecated): {len(unused)}")
for t in sorted(unused):
    in_meta = "✓" if t in meta_tools else "✗"
    print(f"  {t} [metadata: {in_meta}]")