#!/usr/bin/env bash
# Signs MARs with Mozilla's signmar from the build, then checks every signature with
# a database that holds nothing but the certificate, as Whisper's updater will.
#
# The key is a PEM private key (RSA-4096). It goes into an NSS database that exists
# only while this script runs (in a folder only this user can read) and is deleted on
# exit. The key must belong to CERT, or nothing is signed.
#
# Usage: sign-mar.sh KEY.pem CERT.der IN.mar OUT.mar [IN.mar OUT.mar ...]
# Environment:
#   MAR_TOOLS  folder with signmar and mar (the whisper-mar-tools artifact)
#   NSS_LIBS   folder with the NSS libraries signmar was built with: an unpacked Linux
#              package of the same build (libnss3.so, libsoftokn3.so, ...)
# Needs openssl, and certutil and pk12util (Debian/Ubuntu: libnss3-tools).
set -euo pipefail

usage="usage: sign-mar.sh KEY.pem CERT.der IN.mar OUT.mar [IN.mar OUT.mar ...]"
key=${1:?$usage}
cert=${2:?$usage}
shift 2
if [ $# -lt 2 ] || [ $(($# % 2)) != 0 ]; then
  echo "$usage" >&2
  exit 1
fi
tools=${MAR_TOOLS:?set MAR_TOOLS to the folder with signmar}
libs=${NSS_LIBS:?set NSS_LIBS to an unpacked Linux package of the same build}
[ -f "$libs/libnss3.so" ] || { echo "no libnss3.so in $libs" >&2; exit 1; }
chmod +x "$tools/signmar"
signmar() { LD_LIBRARY_PATH="$libs" "$tools/signmar" "$@"; }

# The key must be the private half of CERT.
key_pub=$(openssl pkey -passin pass: -in "$key" -pubout -outform DER | sha256sum | cut -d' ' -f1)
cert_pub=$(openssl x509 -inform DER -in "$cert" -noout -pubkey | openssl pkey -pubin -outform DER \
  | sha256sum | cut -d' ' -f1)
if [ "$key_pub" != "$cert_pub" ]; then
  echo "the signing key does not belong to $(basename "$cert")" \
    "($(openssl x509 -inform DER -in "$cert" -noout -subject))" >&2
  exit 1
fi
bits=$(openssl pkey -passin pass: -in "$key" -noout -text | sed -n 's/.*Private-Key: (\([0-9]*\) bit.*/\1/p')
[ "$bits" = 4096 ] || { echo "expected an RSA-4096 key, got '$bits' bits" >&2; exit 1; }
echo "Signing with the key of $(openssl x509 -inform DER -in "$cert" -noout -subject)"
echo "  certificate SHA-256 $(openssl x509 -inform DER -in "$cert" -noout -fingerprint -sha256 | cut -d= -f2)"

work=$(umask 077 && mktemp -d)
trap 'rm -rf "$work"' EXIT
db="$work/sign"
verify="$work/verify"
mkdir -m 700 "$db" "$verify"
pass=$(openssl rand -hex 24)
openssl x509 -inform DER -in "$cert" -out "$work/cert.pem"
(umask 077 && openssl pkcs12 -export -passin pass: -inkey "$key" -in "$work/cert.pem" -name whisper-mar \
  -passout "pass:$pass" -out "$work/key.p12")
certutil -N -d "sql:$db" --empty-password
pk12util -i "$work/key.p12" -d "sql:$db" -W "$pass" -K "" > /dev/null
rm -f "$work/key.p12"
# The checking database knows only the certificate (no key, no trust flags needed).
certutil -N -d "sql:$verify" --empty-password
certutil -A -d "sql:$verify" -n whisper-mar -t ",," -i "$cert"

while [ $# -gt 0 ]; do
  in=$1
  out=$2
  shift 2
  signmar -d "sql:$db" -n whisper-mar -s "$in" "$out.tmp"
  signmar -d "sql:$verify" -n whisper-mar -v "$out.tmp"
  mv "$out.tmp" "$out"
  echo "signed and verified: $(basename "$out") ($(wc -c < "$out") bytes)"
done
