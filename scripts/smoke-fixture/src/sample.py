"""Install smoke fixture (wicked-estate#360)."""


def py_leaf():
    return 1


def py_caller():
    return py_leaf() + 1
