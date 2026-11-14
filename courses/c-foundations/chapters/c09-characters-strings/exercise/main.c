#include <stdio.h>
#include <string.h>

int main(void)
{
    char word[] = "cat";
    const char target[] = "dog";

    printf("Length: %zu\n", strlen(word));
    printf("Equal: %s\n", strcmp(word, target) == 0 ? "yes" : "no");
    return 0;
}
