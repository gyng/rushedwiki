#!/bin/bash
set -euo pipefail

curl -D /dev/stderr -T ./example.md http://localhost:3000/wiki/test -v
echo "Visit http://localhost:3000/wiki/test"
