func parseOr(_ s: String, fallback: Int) -> Int {
    do {
        return try parse(s)
    } catch {
        return fallback
    }
}
