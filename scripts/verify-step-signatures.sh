#!/usr/bin/env bash
# Generate and verify independent Part 21 CMS signature witnesses.
#
# The argument must be a new directory below $HOME/side2/tmp. The directory
# receives ephemeral keys, certificates, CRLs, exchange files, and verifier
# logs. No generated private key leaves that directory.
set -euo pipefail

usage() {
    echo "usage: $0 $HOME/side2/tmp/<branch>/step-signature-witnesses" >&2
    exit 2
}

[ "$#" -eq 1 ] || usage
out_dir=$1
case "$out_dir" in
    "$HOME"/side2/tmp/*) ;;
    *)
        echo "$0: output must be below $HOME/side2/tmp" >&2
        exit 2
        ;;
esac
if [ -e "$out_dir" ]; then
    echo "$0: output directory already exists: $out_dir" >&2
    exit 2
fi
mkdir -p "$out_dir"

root_dir=$(git rev-parse --show-toplevel)
fixture="$root_dir/crates/cadmpeg-codec-step/src/signature/tests/data/sg04_openssl_detached.p21"
[ -r "$fixture" ] || {
    echo "$0: missing SG-04 source fixture: $fixture" >&2
    exit 1
}

for command_name in base64 fold git openssl python3 tr; do
    command -v "$command_name" >/dev/null || {
        echo "$0: required command is not installed: $command_name" >&2
        exit 1
    }
done

openssl_log() {
    local name=$1
    shift
    "$@" >"$out_dir/$name.stdout" 2>"$out_dir/$name.stderr"
}

expect_success() {
    local name=$1
    shift
    if ! openssl_log "$name" "$@"; then
        echo "$0: expected success: $name" >&2
        cat "$out_dir/$name.stderr" >&2
        exit 1
    fi
}

expect_failure() {
    local name=$1
    shift
    if openssl_log "$name" "$@"; then
        echo "$0: expected failure: $name" >&2
        exit 1
    fi
}

make_root() {
    local name=$1
    local subject=$2
    openssl_log "$name-keygen" openssl genpkey \
        -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
        -out "$out_dir/$name.key"
    openssl_log "$name-certificate" openssl req -x509 -new \
        -key "$out_dir/$name.key" -sha256 -days 30 \
        -subj "$subject" \
        -addext 'basicConstraints=critical,CA:TRUE,pathlen:1' \
        -addext 'keyUsage=critical,keyCertSign,cRLSign' \
        -addext 'subjectKeyIdentifier=hash' \
        -out "$out_dir/$name.pem"
}

make_leaf() {
    local name=$1
    local root_name=$2
    local subject=$3
    local serial=$4
    openssl_log "$name-keygen" openssl genpkey \
        -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
        -out "$out_dir/$name.key"
    openssl_log "$name-request" openssl req -new \
        -key "$out_dir/$name.key" -subj "$subject" \
        -out "$out_dir/$name.csr"
    openssl_log "$name-certificate" openssl x509 -req \
        -in "$out_dir/$name.csr" \
        -CA "$out_dir/$root_name.pem" -CAkey "$out_dir/$root_name.key" \
        -set_serial "$serial" -days 30 -sha256 \
        -extfile "$out_dir/leaf.ext" \
        -out "$out_dir/$name.pem"
}

make_prefix() {
    local label=$1
    local destination=$2
    python3 - "$out_dir/base-prefix.p21" "$label" "$destination" <<'PY'
from pathlib import Path
import sys

source = Path(sys.argv[1]).read_bytes()
old = b"SG-04 OpenSSL detached CMS witness"
if source.count(old) != 1:
    raise SystemExit("SG-04 source marker is not unique")
Path(sys.argv[3]).write_bytes(source.replace(old, sys.argv[2].encode(), 1))
PY
}

make_content() {
    local prefix=$1
    local destination=$2
    python3 - "$prefix" "$destination" <<'PY'
from pathlib import Path
import sys

prefix = Path(sys.argv[1]).read_bytes()
Path(sys.argv[2]).write_bytes(bytes(byte for byte in prefix if byte >= 0x20 and byte != 0x7f))
PY
}

make_cms() {
    local name=$1
    local content=$2
    local signer=$3
    local key=$4
    local certfile=${5-}
    local cert_args=()
    if [ -n "$certfile" ]; then
        cert_args=(-certfile "$certfile")
    fi
    openssl_log "$name-sign" openssl cms -binary -sign \
        -in "$content" -signer "$signer" -inkey "$key" \
        "${cert_args[@]}" -nosmimecap -no_signing_time \
        -outform DER -out "$out_dir/$name.cms"
}

make_exchange() {
    local prefix=$1
    local cms=$2
    local destination=$3
    local encoded="$destination.b64"
    base64 "$cms" | tr -d '\n' | fold -w 64 >"$encoded"
    {
        cat "$prefix"
        printf 'SIGNATURE;\n'
        cat "$encoded"
        printf '\nENDSEC;\n'
    } >"$destination"
}

python3 - "$fixture" "$out_dir/base-prefix.p21" "$out_dir/base-content" <<'PY'
from pathlib import Path
import sys

source = Path(sys.argv[1]).read_bytes()
signature = source.index(b"SIGNATURE;")
prefix = source[:signature]
Path(sys.argv[2]).write_bytes(prefix)
Path(sys.argv[3]).write_bytes(bytes(byte for byte in prefix if byte >= 0x20 and byte != 0x7f))
PY

printf '%s\n' \
    'basicConstraints=critical,CA:FALSE' \
    'keyUsage=critical,digitalSignature' \
    'subjectKeyIdentifier=hash' \
    'authorityKeyIdentifier=keyid,issuer' \
    >"$out_dir/leaf.ext"

# The valid and modified pair use one signer. Only the signed alphabet bytes
# differ in the modified exchange.
make_root valid-root '/CN=cadmpeg SG04 valid root'
make_leaf valid-signer valid-root '/CN=cadmpeg SG04 valid signer' 1001
make_prefix 'SG-04 valid chain witness' "$out_dir/valid-prefix.p21"
make_content "$out_dir/valid-prefix.p21" "$out_dir/valid-content"
make_cms valid "$out_dir/valid-content" "$out_dir/valid-signer.pem" \
    "$out_dir/valid-signer.key" "$out_dir/valid-root.pem"
make_exchange "$out_dir/valid-prefix.p21" "$out_dir/valid.cms" "$out_dir/valid.p21"

make_prefix 'SG-04 tampered chain witness' "$out_dir/modified-prefix.p21"
make_content "$out_dir/modified-prefix.p21" "$out_dir/modified-content"
make_exchange "$out_dir/modified-prefix.p21" "$out_dir/valid.cms" "$out_dir/modified.p21"

# The expired certificate signs detached content successfully. The caller's
# time policy rejects its certificate path.
openssl_log expired-keygen openssl genpkey \
    -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
    -out "$out_dir/expired-signer.key"
openssl_log expired-certificate openssl x509 -new \
    -key "$out_dir/expired-signer.key" \
    -set_subject '/CN=cadmpeg SG04 expired signer' -set_serial 2001 \
    -not_before 20240101000000Z -not_after 20240102000000Z \
    -signkey "$out_dir/expired-signer.key" \
    -out "$out_dir/expired-signer.pem"
make_prefix 'SG-04 expired signer witness' "$out_dir/expired-prefix.p21"
make_content "$out_dir/expired-prefix.p21" "$out_dir/expired-content"
make_cms expired "$out_dir/expired-content" "$out_dir/expired-signer.pem" \
    "$out_dir/expired-signer.key"
make_exchange "$out_dir/expired-prefix.p21" "$out_dir/expired.cms" "$out_dir/expired.p21"

# The revoked signer has a valid CMS signature and a valid chain. The CRL
# check is a separate caller policy input, as the cms command has no CRL-file
# option on the OpenSSL version used by this witness.
make_root revoked-root '/CN=cadmpeg SG04 revocation root'
mkdir -p "$out_dir/revoked-newcerts"
: >"$out_dir/revoked-index.txt"
printf '1000\n' >"$out_dir/revoked-serial"
printf '1000\n' >"$out_dir/revoked-crlnumber"
cat >"$out_dir/revoked-ca.cnf" <<EOF
[ ca ]
default_ca = ca_default
[ ca_default ]
database = $out_dir/revoked-index.txt
serial = $out_dir/revoked-serial
crlnumber = $out_dir/revoked-crlnumber
new_certs_dir = $out_dir/revoked-newcerts
certificate = $out_dir/revoked-root.pem
private_key = $out_dir/revoked-root.key
default_md = sha256
default_days = 30
default_crl_days = 30
policy = policy_any
[ policy_any ]
commonName = supplied
[ revoked_leaf ]
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid,issuer
EOF
openssl_log revoked-signer-keygen openssl genpkey \
    -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
    -out "$out_dir/revoked-signer.key"
openssl_log revoked-signer-request openssl req -new \
    -key "$out_dir/revoked-signer.key" \
    -subj '/CN=cadmpeg SG04 revoked signer' \
    -out "$out_dir/revoked-signer.csr"
openssl_log revoked-signer-certificate openssl ca -batch \
    -config "$out_dir/revoked-ca.cnf" \
    -in "$out_dir/revoked-signer.csr" \
    -out "$out_dir/revoked-signer.pem" -extensions revoked_leaf
openssl_log revoked-signer-revoke openssl ca -batch \
    -config "$out_dir/revoked-ca.cnf" \
    -revoke "$out_dir/revoked-signer.pem" -crl_reason keyCompromise
openssl_log revoked-crl openssl ca -batch \
    -config "$out_dir/revoked-ca.cnf" \
    -gencrl -out "$out_dir/revoked.crl.pem"
make_prefix 'SG-04 revoked signer witness' "$out_dir/revoked-prefix.p21"
make_content "$out_dir/revoked-prefix.p21" "$out_dir/revoked-content"
make_cms revoked "$out_dir/revoked-content" "$out_dir/revoked-signer.pem" \
    "$out_dir/revoked-signer.key" "$out_dir/revoked-root.pem"
make_exchange "$out_dir/revoked-prefix.p21" "$out_dir/revoked.cms" "$out_dir/revoked.p21"

# The unknown-chain signer is cryptographically valid but has no trust anchor
# in the verifier's store. Its root is present only as an untrusted CMS cert.
make_root unknown-root '/CN=cadmpeg SG04 unknown root'
make_leaf unknown-signer unknown-root '/CN=cadmpeg SG04 unknown signer' 3001
make_prefix 'SG-04 unknown chain witness' "$out_dir/unknown-prefix.p21"
make_content "$out_dir/unknown-prefix.p21" "$out_dir/unknown-content"
make_cms unknown "$out_dir/unknown-content" "$out_dir/unknown-signer.pem" \
    "$out_dir/unknown-signer.key" "$out_dir/unknown-root.pem"
make_exchange "$out_dir/unknown-prefix.p21" "$out_dir/unknown.cms" "$out_dir/unknown.p21"

expect_success valid-trusted openssl cms -binary -inform DER -verify \
    -in "$out_dir/valid.cms" -content "$out_dir/valid-content" \
    -CAfile "$out_dir/valid-root.pem" -out "$out_dir/valid.out"
expect_failure modified-content openssl cms -binary -inform DER -verify \
    -in "$out_dir/valid.cms" -content "$out_dir/modified-content" \
    -CAfile "$out_dir/valid-root.pem" -out "$out_dir/modified.out"
expect_success expired-cryptographic openssl cms -binary -inform DER -verify \
    -in "$out_dir/expired.cms" -content "$out_dir/expired-content" \
    -noverify -out "$out_dir/expired-cryptographic.out"
expect_failure expired-policy openssl cms -binary -inform DER -verify \
    -in "$out_dir/expired.cms" -content "$out_dir/expired-content" \
    -CAfile "$out_dir/expired-signer.pem" -out "$out_dir/expired-policy.out"
expect_success revoked-cryptographic openssl cms -binary -inform DER -verify \
    -in "$out_dir/revoked.cms" -content "$out_dir/revoked-content" \
    -noverify -out "$out_dir/revoked-cryptographic.out"
expect_success revoked-chain openssl verify \
    -CAfile "$out_dir/revoked-root.pem" "$out_dir/revoked-signer.pem"
expect_failure revoked-policy openssl verify \
    -crl_check -CAfile "$out_dir/revoked-root.pem" \
    -CRLfile "$out_dir/revoked.crl.pem" "$out_dir/revoked-signer.pem"
expect_success unknown-cryptographic openssl cms -binary -inform DER -verify \
    -in "$out_dir/unknown.cms" -content "$out_dir/unknown-content" \
    -noverify -out "$out_dir/unknown-cryptographic.out"
expect_failure unknown-policy openssl cms -binary -inform DER -verify \
    -in "$out_dir/unknown.cms" -content "$out_dir/unknown-content" \
    -no-CAfile -no-CApath -no-CAstore -out "$out_dir/unknown-policy.out"

printf '%s\n' \
    'valid=valid (content, CMS signature, and trusted certificate path)' \
    'modified=invalid (content verification failed)' \
    'expired=invalid (certificate validity-time policy failed)' \
    'revoked=invalid (certificate revocation policy failed)' \
    'unknown-chain=indeterminate (cryptography passed; no trust anchor)' \
    "witnesses=$out_dir"
