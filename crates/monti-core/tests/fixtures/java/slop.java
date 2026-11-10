class Store {
    void load() {
        try { parse(); } catch (Exception e) {}
        try { parse(); } catch (Exception e) { /* ignore */ }
    }

    Widget build() {
        Widget w = new Widget();
        if (w != null) {
            w.init();
        }
        return w;
    }

    void bump() {
        // increment count
        count++;
    }
}
