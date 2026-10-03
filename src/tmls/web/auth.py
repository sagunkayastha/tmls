"""Use sketchpad's password file for tmls web login."""
import base64
import hashlib
import hmac
import json
import time
from pathlib import Path
from urllib.parse import urlsplit

AUTH_FILE = Path.home() / ".config/sketchpad/auth.json"
SESSION_TTL = 30 * 24 * 3600
COOKIE = "tmls_session"


def load(path=AUTH_FILE):
    try:
        return json.loads(Path(path).read_text())
    except (FileNotFoundError, ValueError):
        return None


def check_login(creds, user, password):
    if not creds:
        return False
    try:
        hashed = hashlib.scrypt(password.encode(), salt=bytes.fromhex(creds["salt"]),
                                n=2**14, r=8, p=1).hex()
        return (hmac.compare_digest(user.encode(), creds["username"].encode())
                and hmac.compare_digest(hashed, creds["hash"]))
    except (KeyError, ValueError, TypeError):
        return False


def _sign(secret, payload):
    # "tmls|": sketchpad signs the same format with the same secret; its cookies must not work here
    return hmac.new(secret.encode(), b"tmls|" + payload.encode(), hashlib.sha256).hexdigest()


def make_cookie(secret, user, now=None):
    now = time.time() if now is None else now
    payload = base64.urlsafe_b64encode(f"{int(now + SESSION_TTL)}|{user}".encode()).decode()
    return f"{payload}.{_sign(secret, payload)}"


SKETCHPAD_COOKIE = "sp_session"


def sketchpad_cookie(secret, user, now=None):
    """sketchpad's own session cookie, in its format (no "tmls|"): both read the same credentials
    file, so a tmls login opens the Sketch tab too. Only this way round: it is not a tmls cookie."""
    now = time.time() if now is None else now
    payload = base64.urlsafe_b64encode(f"{int(now + SESSION_TTL)}|{user}".encode()).decode()
    return f"{payload}.{hmac.new(secret.encode(), payload.encode(), hashlib.sha256).hexdigest()}"


def verify_cookie(secret, value, now=None):
    try:
        payload, sig = value.rsplit(".", 1)
        if not payload or not hmac.compare_digest(sig, _sign(secret, payload)):
            return None
        expires, user = base64.urlsafe_b64decode(payload).decode().split("|", 1)
        now = time.time() if now is None else now
        return user if int(expires) > now else None
    except (AttributeError, ValueError, UnicodeError):
        return None


def same_origin(request):
    origin = request.headers.get("Origin")
    if not origin:
        return True
    url = urlsplit(origin)
    return url.scheme in ("http", "https") and url.netloc == request.host
