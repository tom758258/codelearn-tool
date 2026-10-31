#include <stdio.h>
#include "sum.h"

int main(void) {
    int left = 2;
    int right = 3;
    int total = sum(left, right);
    printf("Sum: %d\n", total);
    return 0;
}
