#!/bin/bash
# Post-edit hook: Run CodeLens diagnostics on the edited file.
# Triggered after Claude Code's Edit tool modifies a file.
#
# Usage in settings.json:
# {
#   "hooks": {
#     "PostToolUse": [{
#       "matcher": "Edit",
#       "command": "./hooks/post-edit-diagnostics.sh \"$TOOL_INPUT_FILE_PATH\""
#     }]
#   }
# }

FILE_PATH="$1"

if [ -z "$FILE_PATH" ]; then
	exit 0
fi

# Only run for supported language files
EXT="${FILE_PATH##*.}"
case "$EXT" in
py | js | ts | tsx | jsx | rs | go | java | kt | cpp | c | rb | php | cs | dart) ;;
*)
	exit 0
	;;
esac

# Run CodeLens diagnostics (oneshot mode)
CODELENS_BIN="${CODELENS_BIN:-codelens-mcp}"
RESULT=$("$CODELENS_BIN" . --cmd get_file_diagnostics --args "{\"file_path\":\"$FILE_PATH\"}" 2>/dev/null)

if [ $? -eq 0 ] && [ -n "$RESULT" ]; then
	# Check if there are actual diagnostics
	COUNT=$(echo "$RESULT" | grep -o '"count":[0-9]*' | head -1 | cut -d: -f2)
	if [ -n "$COUNT" ] && [ "$COUNT" -gt 0 ]; then
		echo "$RESULT"
	fi
fi
