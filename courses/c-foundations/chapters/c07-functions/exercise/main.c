#include <stdio.h>

static int square(int value)
{
    return value * value;
}

int main(void)
{
    int number = 4;
    printf("Square of %d: %d\n", number, square(number));
    return 0;
}
