def load(path):
    try:
        return read_file(path)
    except OSError as e:
        raise RuntimeError(f"cannot load {path}") from e


def total(items):
    # sum() of an empty list is 0, so no guard needed
    subtotal = sum(items)
    tax = subtotal * 0.2
    return subtotal + tax


class Api(Protocol):
    def sign_in(self, pin: str) -> None: ...  # SignInError, LockedOut


class RealHardware:
    def random_bytes(self, n: int) -> bytes:
        return secrets.token_bytes(n)


def defaults(**overrides):
    return {**BASE, **overrides}  # type: ignore[arg-type]
