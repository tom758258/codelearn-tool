#include <stddef.h>
#include <stdio.h>

static int sum_values(const int values[], size_t count)
{
    int sum = 0;
    for (size_t i = 0; i < count; ++i) {
        sum += values[i];
    }
    return sum;
}

int main(void)
{
    const int values[] = {2, 4, 6, 8};
    const size_t count = sizeof(values) / sizeof(values[0]);
    printf("Sum: %d\n", sum_values(values, count));
    return 0;
}
