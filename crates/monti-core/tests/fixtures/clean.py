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
