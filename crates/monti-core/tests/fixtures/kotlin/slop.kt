fun load() {
    try { parse() } catch (e: Exception) {}
    try { parse() } catch (e: Exception) { /* ignore */ }
}

fun bump(counter: Counter) {
    // increment counter
    counter.increment()
}
