#!/bin/bash
cd "$(dirname "$0")"
cat > calc <<'SH'
#!/bin/sh
DIR="$(cd "$(dirname "$0")" && pwd)"
exec python3 -I "$DIR/engine.py" "$@"
SH
chmod +x calc
python3 -c "import ast,sys; ast.parse(open('engine.py').read())" || exit 1
