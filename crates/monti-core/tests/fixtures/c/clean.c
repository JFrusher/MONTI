int clamp(int v, int lo, int hi) {
    // branches, not fmin/fmax: these are ints and must not round-trip through double
    if (v < lo) return lo;
    if (v > hi) return hi;
    return v;
}
