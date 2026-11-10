class Parser {
    int ParseOr(string s, int fallback) {
        try {
            return int.Parse(s);
        } catch (FormatException) {
            return fallback;
        }
    }
}
