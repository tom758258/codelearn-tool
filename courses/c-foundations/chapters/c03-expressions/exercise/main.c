#include <stdio.h>

int main(void)
{
    int a = 7;
    int b = 2;

    printf("Integer: %d\n", a / b);
    printf("Floating: %.1f\n", (double)a / b);
    return 0;
}
