def load(path):
    try:
        return read_file(path)
    except Exception:
        pass


def set_age(user):
    # Set the age
    # to twenty
    user.age = 20


def fetch_user(user_id, opts):
    return api.fetch_user(user_id, opts)


def build():
    items = []
    if items is not None:
        items.append(1)
    return items


class Person:
    def grow(self):
        # increment age
        self.age += 1


def quiet():
    try:
        run()
    except Exception:
        # ignore
        pass


from net import log, send, shape


def report(data, verbose):
    try:
        send(data)
    except Exception as e:
        log(e)
    return shape(data)  # type: ignore
