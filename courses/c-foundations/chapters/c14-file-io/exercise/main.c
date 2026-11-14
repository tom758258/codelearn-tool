#include <stdio.h>

int main(void)
{
    FILE *out = fopen("report.txt", "w");
    if (out == NULL) {
        puts("Cannot open output");
        return 1;
    }
    if (fputs("Stock: 4\n", out) == EOF || fclose(out) == EOF) {
        puts("Cannot write report");
        return 1;
    }

    FILE *in = fopen("report.txt", "r");
    if (in == NULL) {
        puts("Cannot open input");
        return 1;
    }
    char buffer[64];
    if (fgets(buffer, sizeof buffer, in) == NULL) {
        fclose(in);
        puts("Cannot read report");
        return 1;
    }
    fclose(in);
    printf("Read back: %s", buffer);
    return 0;
}
