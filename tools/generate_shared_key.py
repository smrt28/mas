"""Create the local 256-bit build key without printing or replacing it."""
import os
from pathlib import Path
import secrets

path = Path(os.environ.get('MAS_SHARED_KEY_FILE', '.secrets/multiplayer.key'))
path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
try:
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
except FileExistsError:
    raise SystemExit('Key already exists; preserved unchanged.')
with os.fdopen(descriptor, 'wb') as output:
    output.write(secrets.token_bytes(32))
print('Created private build key; its contents were not displayed.')
