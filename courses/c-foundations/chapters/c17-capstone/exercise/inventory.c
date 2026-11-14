#include "inventory.h"

int total_cents(const Product products[], size_t count)
{
    int total = 0;
    for (size_t i = 0; i < count; ++i) {
        total += products[i].quantity * products[i].unit_cents;
    }
    return total;
}
