#include <stddef.h>
#include <stdio.h>

int main(void)
{
    int scores[] = {8, 12, 10, 6};
    size_t count = sizeof(scores) / sizeof(scores[0]);
    int total = 0;
    int maximum = scores[0];

    for (size_t i = 0; i < count; ++i) {
        total += scores[i];
        if (scores[i] > maximum) {
            maximum = scores[i];
        }
    }
    printf("Total: %d\nMax: %d\n", total, maximum);
    return 0;
}
