#!/bin/bash
cd ~/Github/silksurf/silksurf-extras
for browser in dillo Amaya-Editor ladybird elinks-0.13-20251230 links-links2 lynx2.9.2 w3m tkhtml3 sciter servo; do
  echo "=== Scanning $browser ==="
  cd ~/Github/silksurf/silksurf-extras/$browser
  semgrep --config=p/owasp-top-ten --json --output ~/Github/silksurf/diff-analysis/tools-output/semgrep/${browser}.json 2>&1 | grep -A 7 "Scan Summary"
done
