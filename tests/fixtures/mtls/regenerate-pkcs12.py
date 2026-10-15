"""Build public, test-only PKCS#12 fixtures from existing mTLS PEMs (OpenSSL 3).

Tiny DER helpers construct unusual unencrypted test archives that OpenSSL export
cannot produce (mismatched keys and multiple keys). No production code uses them.
"""
from pathlib import Path
import subprocess

BASE = Path(__file__).resolve().parent


def openssl(*args):
    return subprocess.check_output(["openssl", *map(str, args)])


def tlv(tag, value):
    length = len(value)
    size = length.to_bytes((length.bit_length() + 7) // 8, "big")
    return bytes([tag]) + (bytes([length]) if length < 128 else bytes([128 + len(size)]) + size) + value


def seq(*values):
    return tlv(0x30, b"".join(values))


def oid(value):
    numbers = list(map(int, value.split(".")))
    result = bytes([40 * numbers[0] + numbers[1]])
    for number in numbers[2:]:
        parts = [number & 127]
        while number > 127:
            number >>= 7
            parts.insert(0, 128 | (number & 127))
        result += bytes(parts)
    return tlv(6, result)


def content(value):
    return seq(oid("1.2.840.113549.1.7.1"), tlv(0xa0, tlv(4, value)))


def key(name):
    value = openssl("pkcs8", "-topk8", "-nocrypt", "-in", BASE / name, "-outform", "DER")
    return seq(oid("1.2.840.113549.1.12.10.1.1"), tlv(0xa0, value))


def cert(name):
    value = openssl("x509", "-in", BASE / name, "-outform", "DER")
    bag = seq(oid("1.2.840.113549.1.9.22.1"), tlv(0xa0, tlv(4, value)))
    return seq(oid("1.2.840.113549.1.12.10.1.3"), tlv(0xa0, bag))


def archive(name, *bags):
    (BASE / name).write_bytes(seq(tlv(2, b"\x03"), content(seq(content(seq(*bags))))))


archive("client-unencrypted.p12", key("client-key.pem"), cert("client.pem"))
archive("client-chain.p12", cert("ca.pem"), key("client-key.pem"), cert("client.pem"), cert("ca.pem"))
archive("client-mismatch.p12", key("ec-client-key.pem"), cert("client.pem"))
archive("client-multiple.p12", key("client-key.pem"), key("ec-client-key.pem"), cert("client.pem"), cert("ec-client.pem"), cert("ca.pem"))
archive("client-duplicate-key.p12", key("client-key.pem"), key("client-key.pem"), cert("client.pem"))
archive("client-cert-only.p12", cert("client.pem"), cert("ca.pem"))
archive("client-key-only.p12", key("client-key.pem"))
archive("client-unsupported-bag.p12", seq(oid("1.2.840.113549.1.12.10.1.6"), tlv(0xa0, seq(key("ec-client-key.pem")))))
for name, password, options in [
    ("client-protected.p12", "probe-test-only-password", []),
    ("client-empty-password.pfx", "", []),
    ("client-aes128.p12", "probe-test-only-password", ["-keypbe", "AES-128-CBC", "-certpbe", "AES-128-CBC", "-macalg", "sha1"]),
    ("client-legacy.p12", "probe-test-only-password", ["-legacy"]),
    ("client-no-mac.p12", "probe-test-only-password", ["-nomac"]),
]:
    openssl("pkcs12", "-export", "-inkey", BASE / "client-key.pem", "-in", BASE / "client.pem", "-certfile", BASE / "ca.pem", "-out", BASE / name, "-passout", f"pass:{password}", *options)
