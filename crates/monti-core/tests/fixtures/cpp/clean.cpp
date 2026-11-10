int parse_or(const std::string& s, int fallback) {
    try {
        return std::stoi(s);
    } catch (const std::invalid_argument&) {
        return fallback;
    }
}
