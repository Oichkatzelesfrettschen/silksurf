#!/bin/sh
# scripts/measure_idle_cpu.sh -- sample /proc/stat over 5 seconds and print
# the fraction of CPU time spent idle during that window.
#
# WHY: idle_cpu_fraction is a lightweight load-baseline metric for perf
#      records. It flags runs taken under unexpectedly high system load so
#      microbenchmark outliers can be correlated with background activity.
#      Modern CPUs use frequency scaling, so this is advisory rather than a
#      direct energy proxy, but it is far cheaper than a full perf-stat run.
#
# WHAT: reads the aggregate "cpu" line from /proc/stat twice with a 5-second
#       sleep between samples, computes idle_delta / total_delta with awk, and
#       writes a single float (e.g. "0.9234") to stdout.
#
# HOW:
#   sh scripts/measure_idle_cpu.sh
#   python3 perf/append_history.py --idle-cpu $(sh scripts/measure_idle_cpu.sh) ...
#
# Requirements: POSIX sh, awk, /proc/stat (Linux only).
# Not useful on macOS or BSD -- callers should guard with [ -f /proc/stat ].

set -eu

PROC_STAT=/proc/stat

if [ ! -f "$PROC_STAT" ]; then
    echo "measure_idle_cpu.sh: /proc/stat not found -- Linux only" >&2
    exit 1
fi

# Read the aggregate CPU line.
# /proc/stat format (kernel docs, since 2.6.33):
#   cpu  user nice system idle iowait irq softirq steal guest guest_nice
# Fields 2-11 (1-indexed) are tick counts. We want field 5 (idle) and the
# sum of all fields as total. Fields beyond position 11 are not present on
# older kernels; awk tolerates missing columns by treating them as zero.
read_cpu_line() {
    # Print a single line: "idle total" where both are integers.
    awk '/^cpu / {
        idle  = $5
        total = 0
        for (i = 2; i <= NF; i++) total += $i
        print idle, total
        exit
    }' "$PROC_STAT"
}

before=$(read_cpu_line)
sleep 5
after=$(read_cpu_line)

# Compute fraction in awk to stay POSIX sh (no bc, no python dependency here).
echo "$before $after" | awk '{
    idle_before  = $1
    total_before = $2
    idle_after   = $3
    total_after  = $4

    idle_delta  = idle_after  - idle_before
    total_delta = total_after - total_before

    if (total_delta <= 0) {
        print "0.0000"
        exit
    }

    frac = idle_delta / total_delta
    # Clamp to [0, 1] in case of jitter from counter wrap or fast sleep.
    if (frac < 0) frac = 0
    if (frac > 1) frac = 1
    printf "%.4f\n", frac
}'
