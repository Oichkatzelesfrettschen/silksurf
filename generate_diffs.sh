#!/bin/bash

OUTPUT_DIR="./diff-analysis"
mkdir -p "$OUTPUT_DIR"

NS="netsurf-main"
NEO="neosurf-fork"

echo "=== Generating comprehensive diff analysis ==="

# 1. File list analysis
echo "Generating file listings..."
find "$NS" -type f ! -path '*/.git/*' | sort > "$OUTPUT_DIR/netsurf-files.txt"
find "$NEO" -type f ! -path '*/.git/*' | sort > "$OUTPUT_DIR/neosurf-files.txt"

# 2. Relative path analysis
find "$NS" -type f ! -path '*/.git/*' -printf '%P\n' | sort > "$OUTPUT_DIR/netsurf-relative.txt"
find "$NEO" -type f ! -path '*/.git/*' -printf '%P\n' | sort > "$OUTPUT_DIR/neosurf-relative.txt"

# 3. Directory structure
echo "Generating directory trees..."
tree -L 3 "$NS" --dirsfirst 2>/dev/null || find "$NS" -mindepth 1 -maxdepth 3 -type d ! -path '*/.git/*' | sort > "$OUTPUT_DIR/netsurf-tree.txt"
tree -L 3 "$NEO" --dirsfirst 2>/dev/null || find "$NEO" -mindepth 1 -maxdepth 3 -type d ! -path '*/.git/*' | sort > "$OUTPUT_DIR/neosurf-tree.txt"

# 4. File count by extension
echo "Analyzing file types..."
echo "=== NetSurf file types ===" > "$OUTPUT_DIR/file-types.txt"
find "$NS" -type f ! -path '*/.git/*' | sed 's/.*\.//' | sort | uniq -c | sort -rn >> "$OUTPUT_DIR/file-types.txt"
echo "" >> "$OUTPUT_DIR/file-types.txt"
echo "=== NeoSurf file types ===" >> "$OUTPUT_DIR/file-types.txt"
find "$NEO" -type f ! -path '*/.git/*' | sed 's/.*\.//' | sort | uniq -c | sort -rn >> "$OUTPUT_DIR/file-types.txt"

# 5. Build system differences
echo "Analyzing build systems..."
echo "=== NetSurf build files ===" > "$OUTPUT_DIR/build-system.txt"
find "$NS" -maxdepth 2 \( -name "Makefile*" -o -name "*.mk" -o -name "CMakeLists.txt" -o -name "meson.build" \) ! -path '*/.git/*' >> "$OUTPUT_DIR/build-system.txt"
echo "" >> "$OUTPUT_DIR/build-system.txt"
echo "=== NeoSurf build files ===" >> "$OUTPUT_DIR/build-system.txt"
find "$NEO" -maxdepth 2 \( -name "Makefile*" -o -name "*.mk" -o -name "CMakeLists.txt" -o -name "meson.build" \) ! -path '*/.git/*' >> "$OUTPUT_DIR/build-system.txt"

# 6. Size analysis
echo "Generating size analysis..."
du -sh "$NS" > "$OUTPUT_DIR/sizes.txt"
du -sh "$NEO" >> "$OUTPUT_DIR/sizes.txt"
du -sh "$NS"/* >> "$OUTPUT_DIR/sizes.txt"
du -sh "$NEO"/* >> "$OUTPUT_DIR/sizes.txt"

# 7. Common vs unique files
echo "Analyzing file presence..."
comm -23 "$OUTPUT_DIR/netsurf-relative.txt" "$OUTPUT_DIR/neosurf-relative.txt" > "$OUTPUT_DIR/only-in-netsurf.txt"
comm -13 "$OUTPUT_DIR/netsurf-relative.txt" "$OUTPUT_DIR/neosurf-relative.txt" > "$OUTPUT_DIR/only-in-neosurf.txt"
comm -12 "$OUTPUT_DIR/netsurf-relative.txt" "$OUTPUT_DIR/neosurf-relative.txt" > "$OUTPUT_DIR/common-files.txt"

echo "Diff analysis complete. Files generated in $OUTPUT_DIR/"
