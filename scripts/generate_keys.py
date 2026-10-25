#!/usr/bin/env python3
"""Generate an Ed25519 key pair for CN105 HVAC Bridge OTA firmware signing."""

import sys
from pathlib import Path

try:
    from cryptography.hazmat.primitives.asymmetric import ed25519
    from cryptography.hazmat.primitives import serialization
except ImportError:
    sys.stderr.write(
        "Error: 'cryptography' library is not installed.\n"
        "Please install it using: pip3 install cryptography\n"
    )
    sys.exit(1)

KEY_DIR = Path.home() / ".hvac"
KEY_PATH = KEY_DIR / "ota_key.pem"
REPO_ROOT = Path(__file__).resolve().parent.parent
BIN_KEY_PATH = REPO_ROOT / "crates" / "hvac-firmware" / "ota_public_key.bin"


def main():
    KEY_DIR.mkdir(parents=True, exist_ok=True)
    if KEY_PATH.exists():
        print(f"Key already exists at {KEY_PATH}")
        with open(KEY_PATH, "rb") as f:
            private_key = serialization.load_pem_private_key(f.read(), password=None)
    else:
        private_key = ed25519.Ed25519PrivateKey.generate()
        pem = private_key.private_bytes(
            encoding=serialization.Encoding.PEM,
            format=serialization.PrivateFormat.PKCS8,
            encryption_algorithm=serialization.NoEncryption(),
        )
        with open(KEY_PATH, "wb") as f:
            f.write(pem)
        KEY_PATH.chmod(0o600)
        print(f"Generated new Ed25519 private key: {KEY_PATH}")

    public_key = private_key.public_key()
    pub_bytes = public_key.public_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PublicFormat.Raw,
    )

    BIN_KEY_PATH.parent.mkdir(parents=True, exist_ok=True)
    BIN_KEY_PATH.write_bytes(pub_bytes)
    print(f"Wrote 32-byte public key binary to {BIN_KEY_PATH}")


if __name__ == "__main__":
    main()
