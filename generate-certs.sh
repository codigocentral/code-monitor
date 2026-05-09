#!/bin/bash
# Generate self-signed certificates for development

set -e

CERT_DIR="${1:-./certs}"
mkdir -p "$CERT_DIR"

echo "Generating self-signed certificates in $CERT_DIR..."

# Generate CA key and certificate
openssl genrsa -out "$CERT_DIR/ca.key" 4096
openssl req -x509 -new -nodes -key "$CERT_DIR/ca.key" -sha256 -days 3650 \
    -out "$CERT_DIR/ca.crt" \
    -subj "/C=US/ST=State/L=City/O=CodeMonitor/CN=CodeMonitor CA"

# Generate server key
openssl genrsa -out "$CERT_DIR/server.key" 4096

# Generate server CSR
openssl req -new -key "$CERT_DIR/server.key" \
    -out "$CERT_DIR/server.csr" \
    -subj "/C=US/ST=State/L=City/O=CodeMonitor/CN=localhost"

# Create server certificate with SAN
cat > "$CERT_DIR/server.ext" << EOF
authorityKeyIdentifier=keyid,issuer
basicConstraints=CA:FALSE
keyUsage = digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = @alt_names

[alt_names]
DNS.1 = localhost
IP.1 = 127.0.0.1
IP.2 = ::1
EOF

openssl x509 -req -in "$CERT_DIR/server.csr" \
    -CA "$CERT_DIR/ca.crt" -CAkey "$CERT_DIR/ca.key" \
    -CAcreateserial -out "$CERT_DIR/server.crt" \
    -days 365 -sha256 -extfile "$CERT_DIR/server.ext"

# Generate client key
openssl genrsa -out "$CERT_DIR/client.key" 4096

# Generate client CSR
openssl req -new -key "$CERT_DIR/client.key" \
    -out "$CERT_DIR/client.csr" \
    -subj "/C=US/ST=State/L=City/O=CodeMonitor/CN=client"

# Create client certificate
cat > "$CERT_DIR/client.ext" << EOF
authorityKeyIdentifier=keyid,issuer
basicConstraints=CA:FALSE
keyUsage = digitalSignature, keyEncipherment
extendedKeyUsage = clientAuth
EOF

openssl x509 -req -in "$CERT_DIR/client.csr" \
    -CA "$CERT_DIR/ca.crt" -CAkey "$CERT_DIR/ca.key" \
    -CAcreateserial -out "$CERT_DIR/client.crt" \
    -days 365 -sha256 -extfile "$CERT_DIR/client.ext"

# Clean up intermediate files
rm "$CERT_DIR/server.csr" "$CERT_DIR/server.ext"
rm "$CERT_DIR/client.csr" "$CERT_DIR/client.ext"
rm "$CERT_DIR/ca.srl"

# Set appropriate permissions
chmod 600 "$CERT_DIR/server.key" "$CERT_DIR/client.key"
chmod 644 "$CERT_DIR/server.crt" "$CERT_DIR/client.crt"
chmod 644 "$CERT_DIR/ca.crt"

echo ""
echo "✅ Certificates generated successfully in $CERT_DIR/"
echo ""
echo "Files:"
echo "  - ca.crt: Root CA certificate (distribute to clients)"
echo "  - server.crt: Server certificate"
echo "  - server.key: Server private key"
echo "  - client.crt: Client certificate (for mTLS)"
echo "  - client.key: Client private key"
echo ""
