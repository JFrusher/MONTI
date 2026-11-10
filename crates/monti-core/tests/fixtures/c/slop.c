#include <stdio.h>

int add(int a, int b) { return a + b; }

int sum(int a, int b) { return add(a, b); }

void bump(int *counter) {
    // increment counter
    (*counter)++;
}
