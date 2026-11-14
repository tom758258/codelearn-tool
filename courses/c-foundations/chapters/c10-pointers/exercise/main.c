#include <stdio.h>

static void swap(int *left, int *right)
{
    int temporary = *left;
    *left = *right;
    *right = temporary;
}

int main(void)
{
    int a = 3;
    int b = 7;
    printf("Before: %d %d\n", a, b);
    swap(&a, &b);
    printf("After: %d %d\n", a, b);
    return 0;
}
