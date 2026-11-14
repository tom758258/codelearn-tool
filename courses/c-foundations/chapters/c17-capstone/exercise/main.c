#include <stddef.h>
#include <stdio.h>
#include "inventory.h"

int main(void)
{
    const Product products[] = {
        {"Pencil", 3, 15},
        {"Notebook", 2, 60},
    };
    const size_t count = sizeof(products) / sizeof(products[0]);

    for (size_t i = 0; i < count; ++i) {
        printf("%s x%d: %d cents\n",
               products[i].name,
               products[i].quantity,
               products[i].quantity * products[i].unit_cents);
    }
    printf("Total: %d cents\n", total_cents(products, count));
    return 0;
}
