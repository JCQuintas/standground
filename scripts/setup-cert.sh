#!/usr/bin/env bash
set -euo pipefail

# Creates a self-signed code-signing certificate in the login keychain.
# This gives the app a stable identity so macOS TCC can track permissions
# across rebuilds (ad-hoc signing produces a new cdhash every time).
#
# Only needs to be run once per machine.

CERT_NAME="StandGround Dev"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

# Check if certificate already exists
if security find-identity -v -p codesigning 2>/dev/null | grep -q "$CERT_NAME"; then
    echo "Certificate '$CERT_NAME' already exists."
    exit 0
fi

echo "Creating self-signed code-signing certificate '$CERT_NAME'..."

TMPDIR_CERT="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_CERT"' EXIT

# Generate key and self-signed certificate with code-signing extensions
openssl req -x509 -newkey rsa:2048 -nodes \
    -keyout "$TMPDIR_CERT/key.pem" \
    -out "$TMPDIR_CERT/cert.pem" \
    -days 3650 \
    -subj "/CN=$CERT_NAME" \
    -extensions v3_code_sign \
    -config <(cat <<EOF
[req]
distinguished_name = req_dn
x509_extensions = v3_code_sign

[req_dn]
CN = $CERT_NAME

[v3_code_sign]
keyUsage = critical, digitalSignature
extendedKeyUsage = codeSigning
EOF
)

# Bundle into PKCS12 with legacy algorithms compatible with macOS Security framework
PKCS12_PASS="standground-tmp"
openssl pkcs12 -export -inkey "$TMPDIR_CERT/key.pem" -in "$TMPDIR_CERT/cert.pem" \
    -out "$TMPDIR_CERT/cert.p12" -passout "pass:$PKCS12_PASS" \
    -certpbe PBE-SHA1-3DES -keypbe PBE-SHA1-3DES -macalg SHA1

# Import into login keychain (may prompt for keychain password)
echo "Importing into keychain (you may be prompted for your login password)..."
security import "$TMPDIR_CERT/cert.p12" -k "$KEYCHAIN" -T /usr/bin/codesign -P "$PKCS12_PASS"

echo ""
echo "Certificate '$CERT_NAME' created and imported."
echo ""
echo "IMPORTANT: Open Keychain Access, find '$CERT_NAME' under 'login' > 'My Certificates',"
echo "double-click it, expand 'Trust', and set 'Code Signing' to 'Always Trust'."
echo ""
echo "Then verify with:  security find-identity -v -p codesigning"
