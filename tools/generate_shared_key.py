"""Create the local 256-bit build key without printing or replacing it."""
import argparse
import hashlib
import os
from pathlib import Path
import secrets

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument(
    'password', nargs='?', metavar='PASSWORD',
    help='derive the key as SHA-256 of this password encoded as UTF-8; otherwise use random bytes',
)
args = parser.parse_args()

path = Path(os.environ.get('MAS_SHARED_KEY_FILE', '.secrets/multiplayer.key'))
path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
try:
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
except FileExistsError:
    raise SystemExit('Key already exists; preserved unchanged.')
with os.fdopen(descriptor, 'wb') as output:
    key = (hashlib.sha256(args.password.encode('utf-8')).digest()
           if args.password is not None else secrets.token_bytes(32))
    output.write(key)
print('Created private build key; its contents were not displayed.')
