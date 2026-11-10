#include <string>

int add(int a, int b) { return a + b; }

int sum(int a, int b) { return add(a, b); }

void load() {
    try { parse(); } catch (...) {}
    try { parse(); } catch (const std::exception& e) { /* ignore */ }
}

void build() {
    auto w = new Widget();
    if (w != nullptr) {
        w->init();
    }
}
