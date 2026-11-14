#include <stdio.h>

int main(void)
{
    const int limit = 5;
    int sum = 0;
    for (int i = 1; i <= limit; ++i) {
        sum += i;
    }
    printf("Sum 1..%d: %d\n", limit, sum);
    return 0;
}
