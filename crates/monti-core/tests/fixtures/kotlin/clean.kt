fun parseOr(s: String, fallback: Int): Int {
    return try {
        s.toInt()
    } catch (e: NumberFormatException) {
        fallback
    }
}
