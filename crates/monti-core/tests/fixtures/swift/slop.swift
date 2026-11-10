func load() {
    do { try parse() } catch {}
    do { try parse() } catch { /* ignore */ }
}

func bump(counter: Counter) {
    // increment counter
    counter.increment()
}
