#!/usr/bin/env bash
set -euo pipefail

# Exports the "StandGround Dev" certificate as base64 for use in CI secrets.
#
# After running this, add the following GitHub repository secrets:
#   CERTIFICATE_P12       - the base64 output printed below
#   CERTIFICATE_PASSWORD  - the password you enter when exporting
#   SIGN_IDENTITY         - "StandGround Dev"

CERT_NAME="StandGround Dev"
EXPORT_PATH="${1:-standground-dev.p12}"

echo "Exporting '$CERT_NAME' certificate..."
echo "You will be prompted for:"
echo "  1. A password to protect the exported .p12 file (remember this — it becomes CERTIFICATE_PASSWORD)"
echo "  2. Your login keychain password"
echo ""

security export -k ~/Library/Keychains/login.keychain-db \
    -t identities \
    -f pkcs12 \
    -o "$EXPORT_PATH"

echo ""
echo "Exported to: $EXPORT_PATH"
echo ""
echo "Base64 for CERTIFICATE_P12 secret:"
echo "---"
base64 < "$EXPORT_PATH"
echo ""
echo "---"
echo ""
echo "Set these GitHub secrets (Settings > Secrets and variables > Actions):"
echo "  CERTIFICATE_P12      = the base64 above"
echo "  CERTIFICATE_PASSWORD = the password you just entered"
echo "  SIGN_IDENTITY        = $CERT_NAME"
echo ""
echo "Then delete the exported file:"
echo "  rm $EXPORT_PATH"
