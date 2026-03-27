#!/usr/bin/env bash
# Root CA ceremony — generates the offline root CA key and certificate.
#
# This should ideally be run on an air-gapped machine.
# The root CA signs intermediate CA certs for each site's SPIRE server.
#
# Usage:
#   bash ops/scripts/root-ca-ceremony.sh --output /path/to/ca-output
#
# Produces:
#   root-ca.key      — Root CA private key (KEEP OFFLINE)
#   root-ca.crt      — Root CA certificate (distribute to all SPIRE servers)
#   alpha.key         — Intermediate CA key for site alpha
#   alpha.crt         — Intermediate CA cert for site alpha (signed by root)
#   bravo.key         — Intermediate CA key for site bravo
#   bravo.crt         — Intermediate CA cert for site bravo
#   cloud.key         — Intermediate CA key for cloud trust domain
#   cloud.crt         — Intermediate CA cert for cloud

set -euo pipefail

OUTPUT_DIR="${1:---output}"
if [[ "$OUTPUT_DIR" == "--output" ]]; then
    shift 2>/dev/null || true
    OUTPUT_DIR="${1:-./ca-output}"
fi

SITES="${SITES:-alpha bravo cloud}"
ROOT_CN="Desert Bread Root CA"
ROOT_DAYS=3650     # 10 years
INTER_DAYS=1825    # 5 years

mkdir -p "$OUTPUT_DIR"
cd "$OUTPUT_DIR"

echo "=== Desert Bread Root CA Ceremony ==="
echo "Output: $(pwd)"
echo "Sites: $SITES"
echo ""

# 1. Generate Root CA
echo "--- Generating Root CA ---"
openssl ecparam -genkey -name prime256v1 -noout -out root-ca.key
openssl req -new -x509 -key root-ca.key \
    -out root-ca.crt \
    -days "$ROOT_DAYS" \
    -subj "/CN=${ROOT_CN}" \
    -addext "basicConstraints=critical,CA:TRUE" \
    -addext "keyUsage=critical,keyCertSign,cRLSign"

echo "Root CA: root-ca.crt ($(openssl x509 -in root-ca.crt -noout -fingerprint -sha256))"

# 2. Generate Intermediate CAs for each site
for site in $SITES; do
    echo ""
    echo "--- Generating Intermediate CA: ${site} ---"

    domain="${site}.desertbread.net"

    # Generate key
    openssl ecparam -genkey -name prime256v1 -noout -out "${site}.key"

    # Generate CSR
    openssl req -new -key "${site}.key" \
        -out "${site}.csr" \
        -subj "/CN=SPIRE Intermediate CA - ${site}/O=Desert Bread/OU=${site}"

    # Sign with root CA
    openssl x509 -req -in "${site}.csr" \
        -CA root-ca.crt -CAkey root-ca.key \
        -CAcreateserial \
        -out "${site}.crt" \
        -days "$INTER_DAYS" \
        -extfile <(cat <<EOF
basicConstraints=critical,CA:TRUE,pathlen:0
keyUsage=critical,keyCertSign,cRLSign
subjectAltName=URI:spiffe://${domain}
EOF
    )

    rm "${site}.csr"
    echo "Intermediate CA: ${site}.crt (trust domain: spiffe://${domain})"
done

rm -f root-ca.srl

echo ""
echo "=== Ceremony Complete ==="
echo ""
echo "CRITICAL: Store root-ca.key OFFLINE (encrypted USB, HSM, safe)."
echo "          It is only needed to sign new intermediate CA certs."
echo ""
echo "Distribute to each site's SPIRE server:"
for site in $SITES; do
    echo "  ${site}: ${site}.key, ${site}.crt, root-ca.crt"
done
