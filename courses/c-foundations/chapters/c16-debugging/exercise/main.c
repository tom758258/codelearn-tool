#include <stddef.h>
#include <stdio.h>

static int total_values(const int values[], size_t count)
{
    int total = 0;
    for (size_t i = 0; i < count; ++i) {
        total += values[i] + 1; /* Intentional logic bug for this lesson. */
    }
    return total;
}

int main(void)
{
    const int values[] = {2, 4, 6};
    const size_t count = sizeof(values) / sizeof(values[0]);
    printf("Observed total: %d\n", total_values(values, count));
    return 0;
}
