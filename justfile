# List available recipes
default:
    @just --list

# Run CI gates — mirrors .github/workflows/ci.yml; keep the two in step
check: check-default check-local

# Pass 1 — default members: kvscf-core, kvscf-app (remote ON), kvscf
check-default:
    cargo fmt --all --check
    cargo clippy --all-targets -- -D warnings
    cargo build --all-targets
    cargo test

# kvscf-local is excluded from default-members on purpose: a whole-workspace build unifies
# features and would turn `remote` on for the shared kvscf-app. Checking it alone is what
# keeps that path from rotting, and --build-info is the assertion that it really is
# comms-free.
#
# (`just` shows only the LAST comment line above a recipe in `just --list`, so the prose
# stays above the blank line and the doc comment below it.)

# Pass 2 — the no-comms build in isolation (remote OFF)
check-local:
    cargo clippy -p kvscf-local --all-targets -- -D warnings
    cargo build -p kvscf-local
    out="$(cargo run -q -p kvscf-local -- --build-info)"; echo "$out"; echo "$out" | grep -q "remote=false" || { echo "kvscf-local unexpectedly has remote enabled"; exit 1; }

# Apply formatting
fmt:
    cargo fmt --all
