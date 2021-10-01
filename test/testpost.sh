#!/bin/bash
set -euo pipefail

echo "Visit http://localhost:3000/wiki/test"
curl -D /dev/stderr -T ./example.md http://localhost:3000/wiki/test -v
