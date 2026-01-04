#!/bin/bash
# Builds a single consolidated documentation file from all docs/*.md files
# Usage: ./scripts/build-docs.sh [output_file]
# Alias: Add to ~/.bashrc: alias tach-docs='cd /path/to/tach-core && ./scripts/build-docs.sh'

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
DOCS_DIR="$PROJECT_ROOT/docs"
OUTPUT="${1:-$DOCS_DIR/FULL_DOCUMENTATION.md}"

echo "Building consolidated documentation..."

# Create header
cat > "$OUTPUT" << 'EOF'
# Tach-Core Complete Documentation

> Auto-generated from docs/*.md files. Do not edit directly.
> Regenerate with: `./scripts/build-docs.sh`

---

## Table of Contents

EOF

# Generate TOC - Architecture
echo "### Architecture" >> "$OUTPUT"
for f in "$DOCS_DIR"/architecture/*.md; do
    [ -f "$f" ] || continue
    name=$(basename "$f" .md)
    # Skip old phase docs
    [[ "$name" == phase* ]] && continue
    title=$(head -1 "$f" | sed 's/^# //')
    anchor=$(echo "$title" | tr '[:upper:]' '[:lower:]' | tr ' ' '-' | tr -cd '[:alnum:]-')
    echo "- [$title](#$anchor)" >> "$OUTPUT"
done

# Generate TOC - Security
echo "" >> "$OUTPUT"
echo "### Security" >> "$OUTPUT"
for f in "$DOCS_DIR"/security/*.md; do
    [ -f "$f" ] || continue
    title=$(head -1 "$f" | sed 's/^# //')
    anchor=$(echo "$title" | tr '[:upper:]' '[:lower:]' | tr ' ' '-' | tr -cd '[:alnum:]-')
    echo "- [$title](#$anchor)" >> "$OUTPUT"
done

# Generate TOC - Operations
echo "" >> "$OUTPUT"
echo "### Operations" >> "$OUTPUT"
for f in "$DOCS_DIR"/ci/*.md; do
    [ -f "$f" ] || continue
    title=$(head -1 "$f" | sed 's/^# //')
    anchor=$(echo "$title" | tr '[:upper:]' '[:lower:]' | tr ' ' '-' | tr -cd '[:alnum:]-')
    echo "- [$title](#$anchor)" >> "$OUTPUT"
done

# Generate TOC - Decisions
echo "" >> "$OUTPUT"
echo "### Decisions" >> "$OUTPUT"
for f in "$DOCS_DIR"/decisions/*.md; do
    [ -f "$f" ] || continue
    title=$(head -1 "$f" | sed 's/^# //')
    anchor=$(echo "$title" | tr '[:upper:]' '[:lower:]' | tr ' ' '-' | tr -cd '[:alnum:]-')
    echo "- [$title](#$anchor)" >> "$OUTPUT"
done

echo "" >> "$OUTPUT"
echo "### Reference" >> "$OUTPUT"
for f in "$DOCS_DIR"/*.md; do
    [ -f "$f" ] || continue
    name=$(basename "$f" .md)
    # Skip the output file itself
    [[ "$name" == "FULL_DOCUMENTATION" ]] && continue
    title=$(head -1 "$f" | sed 's/^# //')
    anchor=$(echo "$title" | tr '[:upper:]' '[:lower:]' | tr ' ' '-' | tr -cd '[:alnum:]-')
    echo "- [$title](#$anchor)" >> "$OUTPUT"
done

echo "" >> "$OUTPUT"
echo "---" >> "$OUTPUT"
echo "" >> "$OUTPUT"

# Append architecture docs first
echo "# Architecture Documentation" >> "$OUTPUT"
echo "" >> "$OUTPUT"
for f in "$DOCS_DIR"/architecture/*.md; do
    [ -f "$f" ] || continue
    name=$(basename "$f" .md)
    # Skip old phase docs
    [[ "$name" == phase* ]] && continue
    echo "" >> "$OUTPUT"
    cat "$f" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
    echo "---" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
done

# Append security docs
echo "" >> "$OUTPUT"
echo "# Security Documentation" >> "$OUTPUT"
echo "" >> "$OUTPUT"
for f in "$DOCS_DIR"/security/*.md; do
    [ -f "$f" ] || continue
    echo "" >> "$OUTPUT"
    cat "$f" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
    echo "---" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
done

# Append operations docs
echo "" >> "$OUTPUT"
echo "# Operations Documentation" >> "$OUTPUT"
echo "" >> "$OUTPUT"
for f in "$DOCS_DIR"/ci/*.md; do
    [ -f "$f" ] || continue
    echo "" >> "$OUTPUT"
    cat "$f" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
    echo "---" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
done

# Append decision records
echo "" >> "$OUTPUT"
echo "# Architecture Decision Records" >> "$OUTPUT"
echo "" >> "$OUTPUT"
for f in "$DOCS_DIR"/decisions/*.md; do
    [ -f "$f" ] || continue
    echo "" >> "$OUTPUT"
    cat "$f" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
    echo "---" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
done

# Append reference docs
echo "" >> "$OUTPUT"
echo "# Reference Documentation" >> "$OUTPUT"
echo "" >> "$OUTPUT"
for f in "$DOCS_DIR"/*.md; do
    [ -f "$f" ] || continue
    name=$(basename "$f" .md)
    # Skip the output file itself
    [[ "$name" == "FULL_DOCUMENTATION" ]] && continue
    echo "" >> "$OUTPUT"
    cat "$f" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
    echo "---" >> "$OUTPUT"
    echo "" >> "$OUTPUT"
done

# Count stats
LINES=$(wc -l < "$OUTPUT")
FILES=$(find "$DOCS_DIR" -name "*.md" ! -name "FULL_DOCUMENTATION.md" ! -name "phase*.md" | wc -l)

echo ""
echo "Generated: $OUTPUT"
echo "  - $FILES source files"
echo "  - $LINES lines"
echo ""
echo "To set up alias, add to ~/.bashrc:"
echo "  alias tach-docs='$SCRIPT_DIR/build-docs.sh'"
