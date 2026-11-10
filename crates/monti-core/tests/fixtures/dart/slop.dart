void load() {
  try { parse(); } catch (e) {}
  try { parse(); } catch (e) { /* ignore */ }
}

int add(int a, int b) => a + b;

int sum(int a, int b) { return add(a, b); }

void bump(Counter counter) {
  // increment counter
  counter.increment();
}
