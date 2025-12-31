#!/bin/bash
cd ~/Github/silksurf/diff-analysis/tools-output/lizard
echo "=== TOP 10 MOST COMPLEX FUNCTIONS PER BROWSER ===" > top10-complexity.txt
for csv in *.csv; do
  browser=$(basename "$csv" .csv)
  echo "" >> top10-complexity.txt
  echo "### $browser ###" >> top10-complexity.txt
  awk -F',' 'NR>1 {print $2"|"$7"|"$8}' "$csv" | sort -t'|' -k1 -rn | head -10 | column -t -s'|' >> top10-complexity.txt
done
cat top10-complexity.txt
