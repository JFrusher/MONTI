int parseOr(String s, int fallback) {
  try {
    return int.parse(s);
  } on FormatException {
    return fallback;
  }
}
