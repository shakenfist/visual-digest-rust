#!/bin/bash

# Scan this repository's git history for leaked credentials.
#
# Two things happen here, and the second is the more important one:
#
# 1. gitleaks scans every commit reachable from HEAD -- on a pull request
#    that is the whole of develop plus the branch under test -- with its
#    default rules, and the script fails if anything is found.
#
# 2. A positive control proves the scanner can still fire. A scan which
#    reports nothing is indistinguishable from a scan which cannot find
#    anything, so we plant two credentials in a scratch directory and
#    fail unless gitleaks reports both. Green here means "scanned and
#    found nothing", not "did nothing".
#
# Modelled on shakenfist/shakenfist's tools/gitleaks-scan.sh, without
# that repository's custom rules and allowlists.
#
# Usage:
#   tools/gitleaks-scan.sh [--gitleaks PATH]
#
# Runs from anywhere inside the working tree, but the clone must be a
# full one, not shallow.

set -e

GITLEAKS=gitleaks
while [ $# -gt 0 ]; do
    case "$1" in
        --gitleaks)
            if [ -z "$2" ]; then
                echo "--gitleaks needs a path."
                exit 1
            fi
            GITLEAKS="$2"
            shift 2
            ;;
        *)
            # Refuse rather than ignore: a silently discarded flag would
            # leave the caller believing they had changed the scan.
            echo "Unrecognised argument: $1"
            echo "Usage: tools/gitleaks-scan.sh [--gitleaks PATH]"
            exit 1
            ;;
    esac
done

if ! command -v "$GITLEAKS" >/dev/null 2>&1 && [ ! -x "$GITLEAKS" ]; then
    echo "gitleaks not found. Install it, or pass --gitleaks PATH."
    exit 1
fi

echo "Using gitleaks $("$GITLEAKS" version) from $GITLEAKS"

if ! command -v ssh-keygen >/dev/null 2>&1; then
    echo "ssh-keygen is needed to plant the positive control's private key."
    echo "Install openssh-client."
    exit 1
fi

if [ "$(git rev-parse --is-shallow-repository)" = "true" ]; then
    echo "This is a shallow clone, so most of history cannot be scanned."
    echo "Check out with fetch-depth: 0."
    exit 1
fi

cd "$(git rev-parse --show-toplevel)"

# The positive control. Both credentials are generated here rather than
# written into this file, because a literal one would be found by the
# real scan below -- correctly, since a credential in a committed file is
# exactly what we are looking for.
CONTROL=$(mktemp -d)
trap 'rm -rf "$CONTROL"' EXIT

body=$(tr -dc 'A-Za-z0-9' </dev/urandom | head -c 36 || true)
printf 'GITHUB_TOKEN = "ghp_%s"\n' "$body" > "$CONTROL/planted.txt"
ssh-keygen -q -t rsa -b 2048 -N '' -C control@example.com \
    -f "$CONTROL/id_rsa"

echo
echo "Positive control: two credentials planted in a scratch directory."
set +e
"$GITLEAKS" detect --source "$CONTROL" --no-git --redact --no-banner \
    --report-path "$CONTROL/report.json" --report-format json
control_status=$?
set -e

for rule in github-pat private-key; do
    if ! grep -q "\"RuleID\": *\"$rule\"" "$CONTROL/report.json"; then
        echo
        echo "The positive control failed: gitleaks did not report the"
        echo "$rule rule against a credential planted for it to find."
        echo "Do not trust a clean scan until this passes."
        exit 1
    fi
done

if [ $control_status -eq 0 ]; then
    echo "The positive control did not set a failure exit code."
    exit 1
fi

echo "Positive control passed: both planted credentials were reported."
echo

# The real scan. --log-opts="HEAD" scopes it to history reachable from
# this checkout; without it gitleaks scans every ref.
echo "Scanning every commit reachable from HEAD."
"$GITLEAKS" detect --source . --log-opts="HEAD" --redact --no-banner
