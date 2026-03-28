#!/usr/bin/env bash
# Export docs/architecture-diagrams.md to PDF with rendered Mermaid diagrams.
#
# Prerequisites:
#   brew install pandoc
#   brew install --cask mactex  (or basictex)
#   npm install -g @mermaid-js/mermaid-cli
#
# Usage:
#   ./ops/scripts/export-diagrams-pdf.sh
#   ./ops/scripts/export-diagrams-pdf.sh docs/architecture-diagrams.md  # custom input
#   ./ops/scripts/export-diagrams-pdf.sh docs/architecture-diagrams.md output.pdf

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
INPUT="${1:-$REPO_ROOT/docs/architecture-diagrams.md}"
OUTPUT="${2:-$REPO_ROOT/docs/architecture-diagrams.pdf}"
WORK_DIR="$(mktemp -d)"

cleanup() { rm -rf "$WORK_DIR"; }
trap cleanup EXIT

echo "Input:  $INPUT"
echo "Output: $OUTPUT"
echo "Work:   $WORK_DIR"
echo ""

# --- Check prerequisites ---
for cmd in pandoc npx xelatex; do
    if ! command -v "$cmd" &>/dev/null; then
        echo "ERROR: $cmd not found. Install it first." >&2
        exit 1
    fi
done

# --- Copy source to work dir ---
cp "$INPUT" "$WORK_DIR/source.md"

# --- Extract and render Mermaid blocks ---
echo "Rendering Mermaid diagrams..."
count=0
while IFS= read -r line_num; do
    count=$((count + 1))
    start=$line_num

    # Find the closing ```
    end=$(tail -n +$((start + 1)) "$WORK_DIR/source.md" | grep -n '^```$' | head -1 | cut -d: -f1)
    end=$((start + end))

    # Extract mermaid content
    mermaid_file="$WORK_DIR/mermaid_${count}.mmd"
    sed -n "$((start + 1)),$((end - 1))p" "$WORK_DIR/source.md" > "$mermaid_file"

    # Render to PNG
    png_file="$WORK_DIR/mermaid_${count}.png"
    if npx @mermaid-js/mermaid-cli -i "$mermaid_file" -o "$png_file" -b white -w 1200 2>/dev/null; then
        echo "  [$count] rendered (lines $start-$end)"
    else
        echo "  [$count] FAILED (lines $start-$end), skipping"
    fi
done < <(grep -n '```mermaid' "$WORK_DIR/source.md" | cut -d: -f1)

echo "Rendered $count Mermaid diagrams."
echo ""

# --- Replace mermaid blocks with image references (work backwards) ---
cp "$WORK_DIR/source.md" "$WORK_DIR/processed.md"

for ((i=count; i>=1; i--)); do
    png_file="$WORK_DIR/mermaid_${i}.png"
    [ -f "$png_file" ] || continue

    block_start=$(grep -n '```mermaid' "$WORK_DIR/processed.md" | sed -n "${i}p" | cut -d: -f1)
    block_end=$((block_start + 1))
    while ! sed -n "${block_end}p" "$WORK_DIR/processed.md" | grep -q '^```$'; do
        block_end=$((block_end + 1))
    done

    head -n $((block_start - 1)) "$WORK_DIR/processed.md" > "$WORK_DIR/swap.md"
    echo "![](${png_file})" >> "$WORK_DIR/swap.md"
    echo "" >> "$WORK_DIR/swap.md"
    tail -n +$((block_end + 1)) "$WORK_DIR/processed.md" >> "$WORK_DIR/swap.md"
    mv "$WORK_DIR/swap.md" "$WORK_DIR/processed.md"
done

# --- Convert to PDF ---
echo "Generating PDF..."
pandoc "$WORK_DIR/processed.md" \
    -o "$OUTPUT" \
    --pdf-engine=xelatex \
    -V geometry:margin=0.8in \
    -V fontsize=10pt \
    -V mainfont="Helvetica" \
    -V monofont="Menlo" \
    -V linkcolor=blue \
    2>&1 | grep -v "WARNING" || true

size=$(ls -lh "$OUTPUT" | awk '{print $5}')
pages=$(mdls -name kMDItemNumberOfPages "$OUTPUT" 2>/dev/null | awk '{print $3}' || echo "?")
echo ""
echo "Done: $OUTPUT ($size, ${pages} pages)"
