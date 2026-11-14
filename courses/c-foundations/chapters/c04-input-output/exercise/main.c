#include <stdio.h>

int main(void)
{
    int first;
    int second;

    if (scanf("%d %d", &first, &second) != 2) {
        puts("Invalid input");
        return 1;
    }
    printf("Sum: %d\n", first + second);
    return 0;
}
