#include <stdio.h>

int main(void)
{
    int score = 85;
    if (score >= 90) {
        puts("Grade: A");
    } else if (score >= 75) {
        puts("Grade: B");
    } else if (score >= 60) {
        puts("Grade: C");
    } else {
        puts("Grade: F");
    }
    return 0;
}
