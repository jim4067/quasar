#!/usr/bin/env bash
set -euo pipefail

source_dir="${QUASAR_SOURCE:-/workspace/quasar}"
demo_root="${TMPDIR:-/tmp}/quasar-release-cli-smoke"
demo_name="quasar-release-demo"

rm -rf "$demo_root"
mkdir -p "$demo_root"

cd "$demo_root"
quasar init "$demo_name" \
  --yes \
  --no-git \
  --test-language rust \
  --rust-framework quasar-svm \
  --template minimal \
  --toolchain solana

cd "$demo_name"

export QUASAR_SOURCE="$source_dir"
python3 - <<'PY'
import os
import re
from pathlib import Path

manifest = Path("Cargo.toml")
text = manifest.read_text()
replacement = f'quasar-lang = {{ path = "{os.environ["QUASAR_SOURCE"]}/lang" }}'
text, count = re.subn(
    r'quasar-lang = (?:\{ git = "https://github.com/blueshift-gg/quasar", branch = "master" \}|"=[^"]+")',
    replacement,
    text,
)
if count != 1:
    raise SystemExit(f"expected to patch one quasar-lang dependency, patched {count}")
manifest.write_text(text)
PY

quasar build
quasar test
