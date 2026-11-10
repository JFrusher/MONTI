def load(path):
    try:
        return read_file(path)
    except Exception:
        pass


def set_age(user):
    # Set the age
    # to twenty
    user.age = 20
