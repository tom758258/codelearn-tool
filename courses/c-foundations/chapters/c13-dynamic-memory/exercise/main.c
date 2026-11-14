#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>

int main(void)
{
    const size_t count = 4;
    int *values = malloc(count * sizeof *values);
    if (values == NULL) {
        puts("Allocation failed");
        return 1;
    }

    int sum = 0;
    for (size_t i = 0; i < count; ++i) {
        values[i] = (int)(i + 1) * 2;
        sum += values[i];
    }
    printf("Sum: %d\n", sum);
    free(values);
    return 0;
}
