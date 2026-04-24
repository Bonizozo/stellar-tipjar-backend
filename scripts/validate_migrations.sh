#!/usr/bin/env bash
# Validates that every .sql up-migration has a matching .down.sql file.
set -euo pipefail

MIGRATIONS_DIR="${1:-migrations}"
errors=0

for up in "$MIGRATIONS_DIR"/*.sql; do
    [[ "$up" == *.down.sql ]] && continue
    down="${up%.sql}.down.sql"
    if [[ ! -f "$down" ]]; then
        echo "MISSING down migration: $down"
        errors=$((errors + 1))
    fi
done

if [[ $errors -gt 0 ]]; then
    echo "Validation failed: $errors missing down migration(s)."
    exit 1
fi

echo "All migrations have corresponding down files."
