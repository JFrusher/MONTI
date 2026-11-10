class Store {
    void Load() {
        try { Parse(); } catch (Exception e) {}
        try { Parse(); } catch (Exception e) { /* ignore */ }
    }

    void Bump() {
        // increment count
        count++;
    }
}
