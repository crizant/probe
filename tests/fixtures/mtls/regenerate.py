"""Regenerate public test-only mTLS fixtures using OpenSSL 3."""

from pathlib import Path
import subprocess
import tempfile


BASE = Path(__file__).resolve().parent
PASSWORD = "pass:probe-test-only-password"


def openssl(*args):
    subprocess.run(["openssl", *map(str, args)], check=True)


with tempfile.TemporaryDirectory(prefix="probe-mtls-") as temporary:
    scratch = Path(temporary)
    ca_key = scratch / "ca-key.pem"
    openssl(
        "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", ca_key,
        "-out", BASE / "ca.pem", "-subj", "/CN=Probe test-only CA", "-days", "36500",
        "-addext", "basicConstraints=critical,CA:TRUE",
        "-addext", "keyUsage=critical,keyCertSign,cRLSign",
    )
    for name, serial in [("client", 2), ("ec-client", 3), ("server", 4), ("wrong-client", 5)]:
        key = BASE / f"{name}-key.pem"
        certificate = BASE / f"{name}.pem"
        if name == "ec-client":
            openssl("ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", key)
        else:
            openssl("genrsa", "-traditional", "-out", key, "2048")
        csr = scratch / f"{name}.csr"
        openssl("req", "-new", "-key", key, "-out", csr, "-subj", f"/CN=Probe test-only {name}")
        extensions = scratch / f"{name}.ext"
        extensions.write_text(
            "basicConstraints=critical,CA:FALSE\n"
            "keyUsage=critical,digitalSignature,keyEncipherment\n"
            + ("extendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n"
               if name == "server" else "extendedKeyUsage=clientAuth\n")
        )
        if name == "wrong-client":
            openssl(
                "req", "-x509", "-key", key, "-out", certificate,
                "-subj", "/CN=Probe untrusted test-only client", "-days", "36500",
                "-addext", "basicConstraints=critical,CA:FALSE",
                "-addext", "extendedKeyUsage=clientAuth",
            )
        else:
            openssl(
                "x509", "-req", "-in", csr, "-CA", BASE / "ca.pem", "-CAkey", ca_key,
                "-set_serial", serial, "-out", certificate,
                "-days", "365" if name == "server" else "36500", "-extfile", extensions,
            )
    openssl("pkcs8", "-topk8", "-nocrypt", "-in", BASE / "client-key.pem", "-out", BASE / "client-pkcs8-key.pem")
    openssl(
        "pkcs8", "-topk8", "-in", BASE / "client-key.pem", "-out", BASE / "client-encrypted-key.pem",
        "-passout", PASSWORD, "-v2", "aes-256-cbc", "-v2prf", "hmacWithSHA256",
    )
    openssl(
        "rsa", "-in", BASE / "client-key.pem", "-traditional", "-aes256",
        "-out", BASE / "client-legacy-encrypted-key.pem", "-passout", PASSWORD,
    )
    (BASE / "client-chain.pem").write_bytes((BASE / "client.pem").read_bytes() + (BASE / "ca.pem").read_bytes())
